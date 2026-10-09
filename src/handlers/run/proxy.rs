//! Embedded, per-command credential proxy for `stashbase run --proxy` and
//! `stashbase agent run`.
//!
//! The proxy binds only to localhost and lives for the child process lifetime.
//! HTTPS traffic is intercepted with a temporary locally-trusted CA so it can
//! replace Stashbase placeholders in approved request headers before forwarding
//! them to policy-approved destinations. It also enforces ordinary egress rules
//! and records metadata-only audit events for agent sessions.
//!
//! This is experimental local exposure reduction, not a hardened general-purpose
//! proxy or isolation boundary.

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fs::{self, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    task::{Context as TaskContext, Poll},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures_util::{Stream, StreamExt};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{Bytes, Frame, Incoming},
    client::conn::http1 as client_http1,
    header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use log::debug;
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::{
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    ClientConfig, ServerConfig,
};
use rustls_platform_verifier::BuilderVerifierExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use short_uuid::ShortUuid;
use tokio::{
    io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use uuid::Uuid;

use crate::{
    handlers::agent::policy::{
        evaluate_secret_authorization, host_matches, normalize_secret_http_policy,
        SecretAuthorizationDecision, SecretHttpPolicy,
    },
    models::agent::{AgentHttpRuleEffect, AgentMcpRule, SandboxBackend},
    REQUEST_TIMEOUT_SECS,
};

use super::routing::{RemoteMode, RemoteRouting};
#[cfg(test)]
use crate::models::agent::AgentHttpRule;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;
type ProxyFuture = Pin<Box<dyn Future<Output = Result<Response<ProxyBody>, Infallible>> + Send>>;

const AUDIT_LOG_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUDIT_LOG_MAX_FILES: usize = 1_000;
const MCP_INSPECTION_HEADER: &str = "x-stashbase-mcp-inspection";
const DEPENDENCY_HOOK_PATH: &str = "/__stashbase/dependency-check";
const SECRET_SCAN_HOOK_PATH: &str = "/__stashbase/scan";
/// Where a sandboxed git hook asks the proxy to run `stashbase scan` on the host.
pub const SCAN_BROKER_URL_ENV: &str = "STASHBASE_SCAN_BROKER_URL";
const SECRET_SCAN_BODY_LIMIT: usize = 1024 * 1024;

/// Authenticated hooks the proxy serves on the parent's behalf, so the child
/// never holds the Stashbase API key.
pub struct HookBrokerConfig {
    pub api_key: String,
    pub dependency_check: bool,
    /// Enables the secret-scan route.
    pub secret_scan: Option<SecretScanConfig>,
}

pub struct SecretScanConfig {
    /// The run's working directory, scanned on the host.
    pub workdir: PathBuf,
    pub timeout: Duration,
    pub isolation: ScanIsolation,
}

pub enum ScanIsolation {
    /// The scan reads an agent-controlled repository, so it runs with roughly
    /// the agent's own view of the host filesystem.
    Confined(super::scan_sandbox::ScanConfinement),
    /// Runs `exe` directly, for tests of the route itself.
    #[cfg(test)]
    Unconfined { exe: PathBuf },
}

/// An empty home for one scan, so it reads none of the user's dotfiles
/// (git's global config, the CLI's own config) and can still write.
struct ScratchHome(PathBuf);

impl ScratchHome {
    fn create() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("stashbase-scan-home-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for ScratchHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct HookBroker {
    token: String,
    client: reqwest::Client,
    api_key: String,
    dependency_check: bool,
    secret_scan: Option<SecretScanConfig>,
    // One scan at a time: concurrent hooks would race on the same index.
    scan_lock: tokio::sync::Mutex<()>,
}

impl HookBroker {
    fn authorized(&self, request: &Request<Incoming>) -> bool {
        request.method() == Method::POST
            && request.uri().query().is_none()
            && request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value == format!("Bearer {}", self.token))
    }
}

/// One metadata-only event emitted by the local proxy audit log.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ProxyAuditLogEvent {
    pub timestamp: String,
    pub session_id: String,
    pub profile: String,
    /// SHA-256 fingerprint of the normalized policy snapshot for this run.
    pub policy_fingerprint: String,
    /// Opaque local audit-event ID.
    pub event_id: String,
    /// Present only on `session_started`; identifies the selected profile file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_source: Option<String>,
    /// RFC 3339 modification time of the selected profile file at startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_file_modified_at: Option<String>,
    /// SHA-256 of the selected profile file at startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_file_sha256: Option<String>,
    pub action: String,
    /// Present only on `session_started` for remote sessions: `credential` or `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<String>,
    /// How a remote session carried this destination: `remote` or `direct`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    pub destination_host: Option<String>,
    /// Present only for filesystem-policy events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// MCP tool name for MCP tool-call audit events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_tool: Option<String>,
    /// Filesystem operation denied by policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub method: Option<String>,
    #[serde(default)]
    pub binding_name: Option<String>,
    /// Binding origin, such as `secret` or `personal_credential`. This is
    /// metadata only; audit logs never contain binding values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_source: Option<String>,
    pub response_status: Option<u16>,
    pub duration_ms: Option<u64>,
    /// Bytes actually relayed from the child to the destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_bytes: Option<u64>,
    /// Bytes actually relayed from the destination to the child.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_bytes: Option<u64>,
}

/// Exact-match filters for the local audit-log viewer.
#[derive(Debug, Clone, Default)]
pub struct ProxyAuditLogFilter {
    pub profile: Option<String>,
    pub action: Option<String>,
    pub host: Option<String>,
    pub session: Option<String>,
    pub id: Option<String>,
}

impl ProxyAuditLogFilter {
    fn matches(&self, event: &ProxyAuditLogEvent) -> bool {
        self.profile
            .as_ref()
            .is_none_or(|value| value == &event.profile)
            && self
                .action
                .as_ref()
                .is_none_or(|value| value == &event.action)
            && self.host.as_ref().is_none_or(|value| {
                event
                    .destination_host
                    .as_ref()
                    .is_some_and(|host| host == value)
            })
            && self
                .session
                .as_ref()
                .is_none_or(|value| value == &event.session_id)
            && self
                .id
                .as_ref()
                .is_none_or(|value| value == &event.event_id)
    }
}

/// Private, metadata-only audit log for one proxy session.
#[derive(Debug, Clone)]
pub struct ProxyAuditLog {
    session_id: String,
    profile: String,
    policy_fingerprint: String,
    profile_provenance: Option<ProfileAuditProvenance>,
    binding_sources: Arc<HashMap<String, String>>,
    routing: Option<Arc<RwLock<RemoteRouting>>>,
    path: Arc<PathBuf>,
    file: Arc<Mutex<std::fs::File>>,
}

/// Immutable provenance for the profile file selected when an agent session starts.
#[derive(Debug, Clone)]
pub struct ProfileAuditProvenance {
    source: String,
    modified_at: String,
    sha256: String,
}

impl ProfileAuditProvenance {
    pub fn from_file(source: String, path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path).with_context(|| {
            format!(
                "Could not read agent profile metadata from '{}'.",
                path.display()
            )
        })?;
        let modified_at = DateTime::<Utc>::from(metadata.modified().with_context(|| {
            format!(
                "Could not read agent profile modification time from '{}'.",
                path.display()
            )
        })?)
        .to_rfc3339();
        let sha256 = hex::encode(Sha256::digest(fs::read(path).with_context(|| {
            format!("Could not read agent profile file '{}'.", path.display())
        })?));
        Ok(Self {
            source,
            modified_at,
            sha256,
        })
    }
}

impl ProxyAuditLog {
    /// Uses the control-plane session identifier so local metadata can be
    /// correlated with future server-side remote-proxy audit events.
    pub fn local_with_session_id(
        profile: &str,
        session_id: String,
        policy_fingerprint: String,
    ) -> Result<Self> {
        let directory = local_proxy_audit_directory()?;
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        prune_proxy_audit_logs(&directory)?;

        let path = directory.join(format!("agent-{}.jsonl", session_id));
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;

        Ok(Self {
            session_id,
            profile: profile.to_owned(),
            policy_fingerprint,
            profile_provenance: None,
            binding_sources: Arc::new(HashMap::new()),
            routing: None,
            path: Arc::new(path),
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn policy_fingerprint(&self) -> &str {
        &self.policy_fingerprint
    }

    pub fn record_filesystem_denied(&self, path: &str, operation: &str) -> String {
        let id = new_local_audit_event_id();
        self.record_with_bytes(
            "filesystem_denied",
            Some(path),
            None,
            None,
            Some(StatusCode::FORBIDDEN),
            None,
            Some(&id),
            None,
            None,
            Some(path),
            Some(operation),
            None,
        );
        id
    }

    pub fn with_profile_provenance(mut self, profile_provenance: ProfileAuditProvenance) -> Self {
        self.profile_provenance = Some(profile_provenance);
        self
    }

    /// Adds non-sensitive binding origin metadata for remote audit events.
    pub fn with_binding_sources(mut self, binding_sources: HashMap<String, String>) -> Self {
        self.binding_sources = Arc::new(binding_sources);
        self
    }

    /// Records the routing of a remote session: its mode on `session_started`
    /// and, for each event with a destination, whether it went remote or direct.
    pub fn with_routing(mut self, routing: Arc<RwLock<RemoteRouting>>) -> Self {
        self.routing = Some(routing);
        self
    }

    fn record(
        &self,
        action: &str,
        host: Option<&str>,
        method: Option<&Method>,
        secret_name: Option<&str>,
        status: Option<StatusCode>,
        duration: Option<Duration>,
        id: Option<&str>,
    ) {
        self.record_with_bytes(
            action,
            host,
            method,
            secret_name,
            status,
            duration,
            id,
            None,
            None,
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn record_with_bytes(
        &self,
        action: &str,
        host: Option<&str>,
        method: Option<&Method>,
        secret_name: Option<&str>,
        status: Option<StatusCode>,
        duration: Option<Duration>,
        id: Option<&str>,
        request_bytes: Option<u64>,
        response_bytes: Option<u64>,
        path: Option<&str>,
        operation: Option<&str>,
        mcp_tool: Option<&str>,
    ) {
        // Every audit event, including streamed request outcomes, passes
        // through here. Only the action kind is counted, never its details.
        crate::telemetry::count_action(action);
        let event = ProxyAuditLogEvent {
            timestamp: Utc::now().to_rfc3339(),
            session_id: self.session_id.clone(),
            profile: self.profile.clone(),
            policy_fingerprint: self.policy_fingerprint.clone(),
            event_id: id
                .map(str::to_owned)
                .unwrap_or_else(new_local_audit_event_id),
            profile_source: (action == "session_started")
                .then(|| {
                    self.profile_provenance
                        .as_ref()
                        .map(|provenance| provenance.source.clone())
                })
                .flatten(),
            profile_file_modified_at: (action == "session_started")
                .then(|| {
                    self.profile_provenance
                        .as_ref()
                        .map(|provenance| provenance.modified_at.clone())
                })
                .flatten(),
            profile_file_sha256: (action == "session_started")
                .then(|| {
                    self.profile_provenance
                        .as_ref()
                        .map(|provenance| provenance.sha256.clone())
                })
                .flatten(),
            action: action.to_owned(),
            routing_mode: (action == "session_started")
                .then(|| {
                    self.routing
                        .as_ref()
                        .and_then(|routing| routing.read().ok())
                        .map(|routing| routing.mode.as_str().to_owned())
                })
                .flatten(),
            route: (action != "filesystem_denied")
                .then(|| {
                    let host = host?;
                    let routing = self.routing.as_ref()?.read().ok()?;
                    Some(
                        if routing.is_remote(host) {
                            "remote"
                        } else {
                            "direct"
                        }
                        .to_owned(),
                    )
                })
                .flatten(),
            destination_host: (action != "filesystem_denied")
                .then(|| host)
                .flatten()
                .map(str::to_owned),
            path: path.map(str::to_owned),
            operation: operation.map(str::to_owned),
            mcp_tool: mcp_tool.map(str::to_owned),
            method: method.map(Method::as_str).map(str::to_owned),
            binding_name: secret_name.map(str::to_owned),
            binding_source: secret_name
                .and_then(|name| self.binding_sources.get(name))
                .cloned(),
            response_status: status.map(|status| status.as_u16()),
            duration_ms: duration.and_then(|duration| u64::try_from(duration.as_millis()).ok()),
            request_bytes,
            response_bytes,
        };
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(
                file,
                "{}",
                serde_json::to_string(&event).unwrap_or_default()
            );
        }
    }
}

/// Counts response data as Hyper relays it to the child, then records the final
/// audit event on normal completion or early stream drop.
struct AuditedResponseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    request_bytes: Arc<AtomicU64>,
    response_bytes: u64,
    audit_log: Option<ProxyAuditLog>,
    request_id: String,
    action: &'static str,
    host: Option<String>,
    path: Option<String>,
    method: Method,
    secret_name: Option<String>,
    mcp_tool: Option<String>,
    status: StatusCode,
    started: Instant,
    recorded: bool,
}

impl AuditedResponseStream {
    #[allow(clippy::too_many_arguments)]
    fn new(
        inner: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        request_bytes: Arc<AtomicU64>,
        audit_log: Option<ProxyAuditLog>,
        request_id: String,
        action: &'static str,
        host: Option<String>,
        path: Option<String>,
        method: Method,
        secret_name: Option<String>,
        mcp_tool: Option<String>,
        status: StatusCode,
        started: Instant,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            request_bytes,
            response_bytes: 0,
            audit_log,
            request_id,
            action,
            host,
            path,
            method,
            secret_name,
            mcp_tool,
            status,
            started,
            recorded: false,
        }
    }

    fn record_once(&mut self) {
        if self.recorded {
            return;
        }
        self.recorded = true;
        if let Some(audit_log) = &self.audit_log {
            audit_log.record_with_bytes(
                self.action,
                self.host.as_deref(),
                Some(&self.method),
                self.secret_name.as_deref(),
                Some(self.status),
                Some(self.started.elapsed()),
                Some(&self.request_id),
                Some(self.request_bytes.load(Ordering::Relaxed)),
                Some(self.response_bytes),
                self.path.as_deref(),
                None,
                self.mcp_tool.as_deref(),
            );
        }
    }
}

impl Stream for AuditedResponseStream {
    type Item = Result<Frame<Bytes>, BoxError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.response_bytes = self.response_bytes.saturating_add(chunk.len() as u64);
                Poll::Ready(Some(Ok(Frame::data(chunk))))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(Box::new(error)))),
            Poll::Ready(None) => {
                self.record_once();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for AuditedResponseStream {
    fn drop(&mut self) {
        self.record_once();
    }
}

/// Filters complete SSE lines while retaining only an incomplete trailing line.
/// This keeps long-lived MCP responses incremental instead of waiting for EOF.
struct McpToolsListSseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    pending: Vec<u8>,
    policy: ProxyPolicy,
    host: Option<String>,
    path: String,
    finished: bool,
}

impl McpToolsListSseStream {
    fn new(
        inner: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        policy: ProxyPolicy,
        host: Option<String>,
        path: String,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            pending: Vec::new(),
            policy,
            host,
            path,
            finished: false,
        }
    }

    fn filter_bytes(&self, bytes: &[u8]) -> Bytes {
        filter_mcp_tools_list_sse_response(bytes, &self.policy, self.host.as_deref(), &self.path)
            .expect("serializing an MCP JSON value cannot fail")
    }
}

impl Stream for McpToolsListSseStream {
    type Item = Result<Bytes, reqwest::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        loop {
            match self.inner.as_mut().poll_next(context) {
                Poll::Ready(Some(Ok(chunk))) => {
                    self.pending.extend_from_slice(&chunk);
                    let Some(end) = self.pending.iter().rposition(|byte| *byte == b'\n') else {
                        continue;
                    };
                    let complete = self.pending.drain(..=end).collect::<Vec<_>>();
                    return Poll::Ready(Some(Ok(self.filter_bytes(&complete))));
                }
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(None) if !self.finished => {
                    self.finished = true;
                    if self.pending.is_empty() {
                        return Poll::Ready(None);
                    }
                    let trailing = std::mem::take(&mut self.pending);
                    return Poll::Ready(Some(Ok(self.filter_bytes(&trailing))));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Returns the most recent local audit events, ordered oldest to newest.
pub fn read_local_proxy_audit_logs(
    limit: usize,
    since: Option<Duration>,
    filter: &ProxyAuditLogFilter,
) -> Result<Vec<ProxyAuditLogEvent>> {
    let directory = local_proxy_audit_directory()?;
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let cutoff = since
        .and_then(|duration| chrono::Duration::from_std(duration).ok())
        .map(|duration| Utc::now() - duration);
    let mut events = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("agent-") || !name.ends_with(".jsonl") || !entry.file_type()?.is_file()
        {
            continue;
        }

        let contents = fs::read_to_string(entry.path())?;
        for line in contents.lines() {
            let Ok(event) = serde_json::from_str::<ProxyAuditLogEvent>(line) else {
                continue;
            };
            let timestamp = DateTime::parse_from_rfc3339(&event.timestamp)
                .ok()
                .map(|timestamp| timestamp.with_timezone(&Utc));
            if cutoff.is_some_and(|cutoff| timestamp.is_some_and(|timestamp| timestamp < cutoff)) {
                continue;
            }
            if filter.matches(&event) {
                events.push(event);
            }
        }
    }

    events.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
    if events.len() > limit {
        events.drain(..events.len() - limit);
    }
    Ok(events)
}

fn local_proxy_audit_directory() -> Result<PathBuf> {
    let config_path = crate::config::config::get_config_path()?;
    Ok(config_path
        .parent()
        .context("Stashbase config path has no parent directory")?
        .join("audit"))
}

/// Keeps local audit storage bounded without touching files outside our session naming scheme.
fn prune_proxy_audit_logs(directory: &Path) -> Result<()> {
    let now = SystemTime::now();
    let mut logs = Vec::new();

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with("agent-") || !file_name.ends_with(".jsonl") {
            continue;
        }
        if !entry.file_type()?.is_file() {
            continue;
        }

        let modified = entry.metadata()?.modified()?;
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > AUDIT_LOG_RETENTION)
        {
            fs::remove_file(entry.path())?;
        } else {
            logs.push((entry.path(), modified));
        }
    }

    // Reserve a slot for the session log that is about to be created.
    logs.sort_by_key(|(_, modified)| *modified);
    let excess = logs.len().saturating_sub(AUDIT_LOG_MAX_FILES - 1);
    for (path, _) in logs.into_iter().take(excess) {
        fs::remove_file(path)?;
    }

    Ok(())
}

/// Destination policy for credentials proxied into an agent process.
#[derive(Debug, Clone)]
pub struct ProxyPolicy {
    pub secret_policies: HashMap<String, SecretHttpPolicy>,
    pub secret_injections: HashMap<String, SecretInjection>,
    pub allowed_egress_hosts: HashSet<String>,
    pub denied_hosts: HashSet<String>,
    pub denied_read_paths: Vec<String>,
    pub denied_write_paths: Vec<String>,
    pub allow_network_listeners: bool,
    /// Whether credential-bearing requests must also satisfy egress policy.
    pub egress_hosts_configured: bool,
    pub strict_deny: bool,
    pub mcp_rules: Vec<AgentMcpRule>,
    /// Selects which enforcement backend the sandboxed child runs under.
    pub backend: SandboxBackend,
    /// Docker backend only: a custom image reference to run instead of the
    /// built-in default. Takes priority over `sandbox_dockerfile` if both
    /// are somehow set (profile loading rejects setting both).
    pub sandbox_image: Option<String>,
    /// Docker backend only: path to a custom Dockerfile to build and run
    /// instead of the built-in default image.
    pub sandbox_dockerfile: Option<String>,
    /// Docker backend only: `docker run --memory` value, e.g. "2g". No cap
    /// when unset.
    pub sandbox_memory: Option<String>,
    /// Docker backend only: `docker run --cpus` value, e.g. "1.5". No cap
    /// when unset.
    pub sandbox_cpus: Option<String>,
    /// Run inside a per-session git worktree.
    pub worktree: bool,
    /// With `worktree`: continue this earlier run's worktree (`--resume`)
    /// instead of creating a new one. Where the agent works, not what it
    /// may do — so not part of the fingerprint.
    pub worktree_resume: Option<String>,
    /// Docker backend only: repo-relative directories backed by a per-repo
    /// volume instead of the host's copy (see `append_isolated_path_mounts`).
    pub sandbox_isolated_paths: Vec<String>,
}

/// How a placeholder is represented in a child request and rewritten by the proxy.
#[derive(Debug, Clone)]
pub struct SecretInjection {
    pub header: String,
    pub value_template: String,
}

/// Opaque, control-plane-issued credentials used by the local relay.
/// The token is never placed in the child environment.
#[derive(Clone)]
pub struct RemoteProxyConfig {
    pub proxy_url: String,
    pub session: Arc<RwLock<RemoteProxySessionState>>,
    pub placeholders: HashMap<String, String>,
    /// Maps a profile binding name to the child environment variable name.
    pub child_env: HashMap<String, String>,
    pub protocol: RemoteProxyProtocol,
    /// Key-ID-specific public CA cached from the session response. It is pinned
    /// for this child run so a later CA rotation cannot overwrite its trust file.
    pub ca_file: Option<PathBuf>,
    /// Which hosts use the remote proxy. Replaced atomically on session
    /// rotation; the child cannot influence it.
    pub routing: Arc<RwLock<RemoteRouting>>,
}

/// The currently usable remote session. The rotation task replaces this atomically
/// before a new connection is opened; existing tunnels keep their old session.
#[derive(Clone)]
pub struct RemoteProxySessionState {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub last_rotation_error: Option<String>,
}

impl std::fmt::Debug for RemoteProxySessionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteProxySessionState")
            .field("token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("last_rotation_error", &self.last_rotation_error)
            .finish()
    }
}

impl RemoteProxyConfig {
    /// Whether traffic to `host` goes through the remote proxy. A missing host
    /// or an unreadable table fails toward the remote proxy, never toward a
    /// direct connection.
    fn routes_remotely(&self, host: Option<&str>) -> bool {
        match (self.routing.read(), host) {
            (Ok(routing), Some(host)) => routing.is_remote(host),
            (Ok(routing), None) => routing.mode == RemoteMode::Full,
            (Err(_), _) => true,
        }
    }

    pub fn routing_mode(&self) -> RemoteMode {
        self.routing
            .read()
            .map(|routing| routing.mode)
            .unwrap_or(RemoteMode::Full)
    }

    /// Never call this for an already-open stream: session rotation only applies
    /// to new HTTP requests and CONNECT handshakes.
    fn token_for_new_connection(&self) -> Result<String> {
        let session = self
            .session
            .read()
            .map_err(|_| anyhow::anyhow!("Agent Proxy session state is unavailable"))?;
        if Utc::now() >= session.expires_at {
            let suffix = session
                .last_rotation_error
                .as_deref()
                .map(|error| format!(" (last rotation attempt failed: {error})"))
                .unwrap_or_default();
            anyhow::bail!("Agent Proxy session expired; a new connection cannot be opened{suffix}");
        }
        Ok(session.token.clone())
    }
}

impl std::fmt::Debug for RemoteProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteProxyConfig")
            .field("proxy_url", &self.proxy_url)
            .field("session", &"[REDACTED]")
            .field("placeholders", &self.placeholders)
            .field("child_env", &self.child_env)
            .field("protocol", &self.protocol)
            .field("ca_file", &self.ca_file)
            .field("routing_mode", &self.routing_mode())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProxyProtocol {
    Custom,
    ForwardProxyTlsIntercept,
}

impl SecretInjection {
    pub fn bearer() -> Self {
        Self {
            header: "authorization".to_owned(),
            value_template: "Bearer {value}".to_owned(),
        }
    }
}

impl ProxyPolicy {
    /// What kind of agent worktree this run wants, if any.
    pub(crate) fn worktree_request(&self) -> Option<super::worktree::WorktreeRequest> {
        self.worktree.then(|| match &self.worktree_resume {
            Some(name) => super::worktree::WorktreeRequest::Resume(name.clone()),
            None => super::worktree::WorktreeRequest::New,
        })
    }

    pub fn permissive() -> Self {
        Self {
            secret_policies: HashMap::new(),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: false,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        }
    }

    /// Stable SHA-256 identifier for this policy's effective normalized form.
    /// It deliberately excludes secret values and child placeholders.
    pub fn fingerprint(&self) -> String {
        let mut lines = vec![
            format!("egress_configured={}", self.egress_hosts_configured),
            format!("strict_deny={}", self.strict_deny),
            format!("allow_network_listeners={}", self.allow_network_listeners),
            // The sandbox backend and its Docker-specific settings are part
            // of the effective enforcement, not just the egress/secret
            // policy — switching a profile from native to Docker, or
            // swapping its custom image, must change the fingerprint, or
            // an audit record can't tell those materially different runs
            // apart.
            format!("backend={:?}", self.backend),
            format!(
                "sandbox_image={}",
                self.sandbox_image.as_deref().unwrap_or("")
            ),
            format!(
                "sandbox_dockerfile={}",
                self.sandbox_dockerfile.as_deref().unwrap_or("")
            ),
            format!(
                "sandbox_memory={}",
                self.sandbox_memory.as_deref().unwrap_or("")
            ),
            format!(
                "sandbox_cpus={}",
                self.sandbox_cpus.as_deref().unwrap_or("")
            ),
            format!("worktree={}", self.worktree),
            format!(
                "sandbox_isolated_paths={}",
                self.sandbox_isolated_paths.join(",")
            ),
        ];
        let mut egress = normalize_hosts(self.allowed_egress_hosts.clone())
            .into_iter()
            .collect::<Vec<_>>();
        egress.sort();
        lines.push(format!("egress={}", egress.join(",")));
        let mut denied = normalize_hosts(self.denied_hosts.clone())
            .into_iter()
            .collect::<Vec<_>>();
        denied.sort();
        lines.push(format!("deny={}", denied.join(",")));
        lines.push(format!("deny_read={}", self.denied_read_paths.join(",")));
        lines.push(format!("deny_write={}", self.denied_write_paths.join(",")));

        let mut secret_policies = self.secret_policies.iter().collect::<Vec<_>>();
        secret_policies.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (name, policy) in secret_policies {
            match normalize_secret_http_policy(policy.clone()) {
                SecretHttpPolicy::LegacyHosts(hosts) => {
                    let mut hosts = hosts.into_iter().collect::<Vec<_>>();
                    hosts.sort();
                    lines.push(format!("secret={name};legacy={}", hosts.join(",")));
                }
                SecretHttpPolicy::Rules(rules) => {
                    let mut rules = rules
                        .into_iter()
                        .map(|rule| {
                            let mut hosts = rule.hosts;
                            let mut methods = rule.methods;
                            let mut paths = rule.paths;
                            hosts.sort();
                            methods.sort();
                            paths.sort();
                            format!(
                                "{:?};{};{};{}",
                                rule.effect,
                                hosts.join(","),
                                methods.join(","),
                                paths.join(",")
                            )
                        })
                        .collect::<Vec<_>>();
                    rules.sort();
                    lines.push(format!("secret={name};rules={}", rules.join("|")));
                }
            }
        }
        let mut injections = self.secret_injections.iter().collect::<Vec<_>>();
        injections.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (name, injection) in injections {
            lines.push(format!(
                "injection={name};{};{}",
                injection.header.to_ascii_lowercase(),
                injection.value_template
            ));
        }
        let mut mcp_rules = self
            .mcp_rules
            .iter()
            .map(|rule| {
                let mut hosts = rule
                    .hosts
                    .iter()
                    .map(|host| host.trim().to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let mut paths = rule
                    .paths
                    .iter()
                    .map(|path| path.trim().to_owned())
                    .collect::<Vec<_>>();
                let mut tools = rule.tools.clone();
                hosts.sort();
                paths.sort();
                tools.sort();
                format!(
                    "{:?};{};{};{}",
                    rule.effect,
                    hosts.join(","),
                    paths.join(","),
                    tools.join(",")
                )
            })
            .collect::<Vec<_>>();
        mcp_rules.sort();
        lines.push(format!("mcp_rules={}", mcp_rules.join("|")));

        hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
    }
}

#[derive(Clone)]
struct ProxyState {
    secrets: Arc<HashMap<String, String>>,
    policy: ProxyPolicy,
    client: reqwest::Client,
    remote_ca: Option<reqwest::Certificate>,
    certificate_authority: Arc<CertifiedIssuer<'static, KeyPair>>,
    audit_log: Option<ProxyAuditLog>,
    connections: Arc<ActiveConnections>,
    remote: Option<RemoteProxyConfig>,
    mcp_inspection_token: String,
    hook_broker: Option<Arc<HookBroker>>,
    revocation_path: Arc<RwLock<Option<PathBuf>>>,
}

impl ProxyState {
    /// The remote proxy config when this destination must go remote; `None`
    /// for a local session or a host that credential routing sends direct.
    fn remote_for(&self, host: Option<&str>) -> Option<RemoteProxyConfig> {
        self.remote
            .as_ref()
            .filter(|remote| remote.routes_remotely(host))
            .cloned()
    }
}

/// Tracks every accepted proxy and TLS-upgrade task so proxy shutdown closes
/// existing sockets as well as the listening socket.
#[derive(Default)]
struct ActiveConnections {
    inner: Mutex<ActiveConnectionState>,
}

#[derive(Default)]
struct ActiveConnectionState {
    stopped: bool,
    tasks: Vec<JoinHandle<()>>,
}

impl ActiveConnections {
    fn track(&self, task: JoinHandle<()>) {
        let mut state = self.inner.lock().expect("proxy connection lock poisoned");
        if state.stopped {
            task.abort();
        } else {
            state.tasks.push(task);
        }
    }

    fn stop(&self) {
        let mut state = self.inner.lock().expect("proxy connection lock poisoned");
        state.stopped = true;
        for task in state.tasks.drain(..) {
            task.abort();
        }
    }

    fn close_existing(&self) {
        let mut state = self.inner.lock().expect("proxy connection lock poisoned");
        for task in state.tasks.drain(..) {
            task.abort();
        }
    }
}

/// Owns the listener and the temporary trust anchor for exactly one child process.
pub struct Proxy {
    child_env: HashMap<String, String>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    // Keeping this file alive makes the CA available to the child. Drop removes it.
    ca_file: PathBuf,
    remove_ca_file: bool,
    audit_log: Option<ProxyAuditLog>,
    connections: Arc<ActiveConnections>,
    mcp_inspection_token: String,
    revocation_path: Arc<RwLock<Option<PathBuf>>>,
}

impl Proxy {
    #[cfg(test)]
    pub async fn start(
        secrets: HashMap<String, String>,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
    ) -> Result<Self> {
        Self::start_with_port(secrets, policy, audit_log, None).await
    }

    pub async fn start_with_port(
        secrets: HashMap<String, String>,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
    ) -> Result<Self> {
        Self::start_inner(
            secrets,
            policy,
            audit_log,
            proxy_port,
            None,
            None,
            false,
            "127.0.0.1",
        )
        .await
    }

    pub async fn start_with_hook(
        secrets: HashMap<String, String>,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
        hooks: Option<HookBrokerConfig>,
    ) -> Result<Self> {
        Self::start_inner(
            secrets,
            policy,
            audit_log,
            proxy_port,
            None,
            hooks,
            true,
            "127.0.0.1",
        )
        .await
    }

    /// Like `start_with_hook`, but binds the proxy to `bind_host` instead of
    /// loopback. Used by the Docker sandbox backend, which binds the proxy
    /// to a per-run Docker network's gateway address so only the sandboxed
    /// container (not the whole LAN) can reach it.
    pub async fn start_with_hook_and_bind_host(
        secrets: HashMap<String, String>,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
        hooks: Option<HookBrokerConfig>,
        bind_host: &str,
    ) -> Result<Self> {
        Self::start_inner(
            secrets, policy, audit_log, proxy_port, None, hooks, true, bind_host,
        )
        .await
    }

    pub async fn start_remote_with_port(
        remote: RemoteProxyConfig,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
    ) -> Result<Self> {
        let placeholders = remote.placeholders.clone();
        Self::start_inner(
            placeholders,
            policy,
            audit_log,
            proxy_port,
            Some(remote),
            None,
            false,
            "127.0.0.1",
        )
        .await
    }

    pub async fn start_remote_with_hook(
        remote: RemoteProxyConfig,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
        hooks: Option<HookBrokerConfig>,
    ) -> Result<Self> {
        let placeholders = remote.placeholders.clone();
        Self::start_inner(
            placeholders,
            policy,
            audit_log,
            proxy_port,
            Some(remote),
            hooks,
            true,
            "127.0.0.1",
        )
        .await
    }

    /// Like `start_remote_with_hook`, but binds the proxy to `bind_host`
    /// instead of loopback. See `start_with_hook_and_bind_host`.
    pub async fn start_remote_with_hook_and_bind_host(
        remote: RemoteProxyConfig,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
        hooks: Option<HookBrokerConfig>,
        bind_host: &str,
    ) -> Result<Self> {
        let placeholders = remote.placeholders.clone();
        Self::start_inner(
            placeholders,
            policy,
            audit_log,
            proxy_port,
            Some(remote),
            hooks,
            true,
            bind_host,
        )
        .await
    }

    async fn start_inner(
        secrets: HashMap<String, String>,
        mut policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
        remote: Option<RemoteProxyConfig>,
        hooks: Option<HookBrokerConfig>,
        hook_mode_set: bool,
        bind_host: &str,
    ) -> Result<Self> {
        if proxy_port == Some(0) {
            anyhow::bail!("--proxy-port must be between 1 and 65535");
        }
        let (certificate_authority, mut ca_file) = create_certificate_authority()?;
        let mut remove_ca_file = true;
        let mut remote_ca_file = None;
        if remote
            .as_ref()
            .is_some_and(|remote| remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept)
        {
            let remote_ca = remote
                .as_ref()
                .and_then(|remote| remote.ca_file.clone())
                .context("remote forward-proxy session did not provide a cached CA file")?;
            if remote
                .as_ref()
                .is_some_and(|remote| remote.routing_mode() == RemoteMode::Credential)
            {
                // Remote-routed hosts are intercepted by the remote proxy, but
                // direct hosts are still intercepted by this relay. The child
                // must trust both CAs, so extend our temporary CA file (which
                // is removed on shutdown) with the remote proxy's public CA.
                let mut bundle = fs::read(&ca_file)?;
                bundle.push(b'\n');
                bundle.extend(fs::read(&remote_ca)?);
                fs::write(&ca_file, bundle).context("failed to write the proxy CA bundle")?;
            } else {
                // The remote listener, not this local relay, presents
                // certificates in full forward-proxy mode. Pass its public CA
                // to the child.
                let _ = fs::remove_file(&ca_file);
                ca_file = remote_ca.clone();
                remove_ca_file = false;
            }
            remote_ca_file = Some(remote_ca);
        }
        let bind_address = format!("{bind_host}:{}", proxy_port.unwrap_or(0));
        let listener = TcpListener::bind(&bind_address)
            .await
            .with_context(|| format!("failed to bind credential proxy to {bind_address}"))?;
        let address = listener.local_addr()?;
        let placeholders = if remote.is_some() {
            secrets
                .into_iter()
                .map(|(_name, placeholder)| (placeholder, String::new()))
                .collect()
        } else {
            secrets
                .into_iter()
                .map(|(name, value)| (placeholder_for(&name), value))
                .collect()
        };
        let remote_placeholders = remote.as_ref().map(|remote| remote.placeholders.clone());
        let remote_child_env = remote.as_ref().map(|remote| remote.child_env.clone());
        let remote_binding_names = remote_placeholders.as_ref().map(|placeholders| {
            placeholders
                .iter()
                .map(|(name, placeholder)| (placeholder.clone(), name.clone()))
                .collect::<HashMap<_, _>>()
        });
        policy.secret_policies = policy
            .secret_policies
            .into_iter()
            .map(|(name, secret_policy)| {
                let placeholder = remote_placeholders
                    .as_ref()
                    .and_then(|placeholders| placeholders.get(&name))
                    .cloned()
                    .unwrap_or_else(|| placeholder_for(&name));
                (placeholder, normalize_secret_http_policy(secret_policy))
            })
            .collect();
        policy.secret_injections =
            normalize_injections(policy.secret_injections, remote_placeholders.as_ref())?;
        policy.allowed_egress_hosts = normalize_hosts(policy.allowed_egress_hosts);
        policy.denied_hosts = normalize_hosts(policy.denied_hosts);
        let connections = Arc::new(ActiveConnections::default());
        let mut client_builder = reqwest::Client::builder()
            .no_proxy()
            // A total request timeout would terminate healthy long-lived streams.
            // Keep the existing timeout budget for connecting and for each stalled
            // read instead, so active uploads, downloads, and SSE can continue.
            .connect_timeout(Duration::from_secs(
                REQUEST_TIMEOUT_SECS.get().copied().unwrap_or(30),
            ))
            .read_timeout(Duration::from_secs(
                REQUEST_TIMEOUT_SECS.get().copied().unwrap_or(30),
            ))
            .redirect(reqwest::redirect::Policy::none());
        let remote_ca = if let Some(remote_ca_file) = &remote_ca_file {
            let remote_ca = reqwest::Certificate::from_pem(&fs::read(remote_ca_file)?)?;
            client_builder = client_builder.add_root_certificate(remote_ca.clone());
            Some(remote_ca)
        } else {
            None
        };
        let revocation_path = Arc::new(RwLock::new(None));
        let state = ProxyState {
            secrets: Arc::new(placeholders),
            policy,
            // Forwarding must never use proxy variables inherited by Stashbase itself.
            client: client_builder.build()?,
            remote_ca,
            certificate_authority: Arc::new(certificate_authority),
            audit_log: audit_log.clone(),
            connections: connections.clone(),
            remote,
            mcp_inspection_token: Uuid::new_v4().to_string(),
            hook_broker: hooks.map(|hooks| {
                Arc::new(HookBroker {
                    token: Uuid::new_v4().to_string(),
                    client: reqwest::Client::builder()
                        .no_proxy()
                        .default_headers(reqwest::header::HeaderMap::from_iter([(
                            reqwest::header::AUTHORIZATION,
                            format!("Bearer {}", hooks.api_key).parse().unwrap(),
                        )]))
                        .build()
                        .expect("hook broker client must build"),
                    api_key: hooks.api_key,
                    dependency_check: hooks.dependency_check,
                    secret_scan: hooks.secret_scan,
                    scan_lock: tokio::sync::Mutex::new(()),
                })
            }),
            revocation_path: revocation_path.clone(),
        };
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(run_listener(listener, state.clone(), shutdown_rx));

        let proxy_url = format!("http://{address}");
        let ca_path = ca_file.to_string_lossy().into_owned();
        let mut child_env = HashMap::from([
            ("HTTP_PROXY".to_owned(), proxy_url.clone()),
            ("HTTPS_PROXY".to_owned(), proxy_url),
            ("http_proxy".to_owned(), format!("http://{address}")),
            ("https_proxy".to_owned(), format!("http://{address}")),
            // Common TLS clients honor one of these paths. Tools that do not cannot use
            // HTTPS interception without manually trusting the temporary CA.
            ("SSL_CERT_FILE".to_owned(), ca_path.clone()),
            ("CURL_CA_BUNDLE".to_owned(), ca_path.clone()),
            ("GIT_SSL_CAINFO".to_owned(), ca_path.clone()),
            ("NODE_EXTRA_CA_CERTS".to_owned(), ca_path),
            (
                "CODEX_CA_CERTIFICATE".to_owned(),
                ca_file.to_string_lossy().into_owned(),
            ),
            // Node's built-in fetch requires this opt-in before it reads proxy variables.
            ("NODE_USE_ENV_PROXY".to_owned(), "1".to_owned()),
            ("NO_PROXY".to_owned(), String::new()),
            ("no_proxy".to_owned(), String::new()),
            // Marks every agent session (native, Docker and remote) so a
            // Stashbase CLI run by the agent never sends telemetry, whatever
            // the profile's egress policy allows.
            ("STASHBASE_SANDBOX".to_owned(), "1".to_owned()),
        ]);
        for placeholder in state.secrets.keys() {
            let env_name = child_env_name_for_placeholder(
                placeholder,
                remote_binding_names.as_ref(),
                remote_child_env.as_ref(),
            );
            child_env.insert(env_name, placeholder.clone());
        }
        if hook_mode_set {
            child_env.insert(
                crate::api::dependencies::HOOK_MODE_ENV.to_owned(),
                if state.hook_broker.is_some() {
                    "broker"
                } else {
                    "disabled"
                }
                .to_owned(),
            );
            if let Some(broker) = &state.hook_broker {
                if broker.dependency_check {
                    child_env.insert(
                        crate::api::dependencies::HOOK_BROKER_URL_ENV.to_owned(),
                        format!("http://{address}{DEPENDENCY_HOOK_PATH}"),
                    );
                }
                if broker.secret_scan.is_some() {
                    child_env.insert(
                        SCAN_BROKER_URL_ENV.to_owned(),
                        format!("http://{address}{SECRET_SCAN_HOOK_PATH}"),
                    );
                }
                child_env.insert(
                    crate::api::dependencies::HOOK_BROKER_TOKEN_ENV.to_owned(),
                    broker.token.clone(),
                );
            }
        }

        if let Some(audit_log) = &audit_log {
            audit_log.record("session_started", None, None, None, None, None, None);
        }

        Ok(Self {
            child_env,
            shutdown: Some(shutdown),
            task: Some(task),
            ca_file,
            remove_ca_file,
            audit_log,
            connections,
            mcp_inspection_token: state.mcp_inspection_token.clone(),
            revocation_path,
        })
    }

    pub fn child_env(&self) -> &HashMap<String, String> {
        &self.child_env
    }

    pub fn mcp_inspection_token(&self) -> &str {
        &self.mcp_inspection_token
    }

    pub fn set_revocation_path(&self, path: PathBuf) {
        *self
            .revocation_path
            .write()
            .expect("proxy revocation path lock poisoned") = Some(path);
    }

    pub async fn stop(mut self) {
        self.connections.stop();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        if let Some(audit_log) = &self.audit_log {
            audit_log.record("session_stopped", None, None, None, None, None, None);
        }
    }

    pub fn trust_ca(&self) -> Result<super::trust::TemporaryCaTrust> {
        super::trust::install(&self.ca_file)
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.connections.stop();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if self.remove_ca_file {
            let _ = std::fs::remove_file(&self.ca_file);
        }
    }
}

fn placeholder_for(name: &str) -> String {
    format!("**STASHBASE_{name}**")
}

fn secret_name_from_placeholder(placeholder: &str) -> String {
    if let Some(value) = placeholder
        .strip_prefix("**STASHBASE_")
        .and_then(|value| value.strip_suffix("**"))
    {
        return value.to_owned();
    }
    placeholder
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(placeholder)
        .to_owned()
}

fn child_env_name_for_placeholder(
    placeholder: &str,
    remote_binding_names: Option<&HashMap<String, String>>,
    remote_child_env: Option<&HashMap<String, String>>,
) -> String {
    let binding_name = remote_binding_names
        .and_then(|names| names.get(placeholder))
        .cloned()
        .unwrap_or_else(|| secret_name_from_placeholder(placeholder));
    remote_child_env
        .and_then(|child_env| child_env.get(&binding_name))
        .cloned()
        .unwrap_or(binding_name)
}

fn create_certificate_authority() -> Result<(CertifiedIssuer<'static, KeyPair>, PathBuf)> {
    let subject = format!("Stashbase Proxy {}", Uuid::new_v4());
    let mut params = CertificateParams::new(vec!["stashbase-proxy.local".to_owned()])?;
    params
        .distinguished_name
        .push(DnType::CommonName, subject.clone());
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let ca = CertifiedIssuer::self_signed(params, KeyPair::generate()?)?;
    let path = std::env::temp_dir().join(format!("stashbase-proxy-ca-{}.pem", Uuid::new_v4()));
    std::fs::write(&path, ca.pem()).context("failed to write temporary proxy CA")?;
    Ok((ca, path))
}

/// Returns the control-plane CA used by the standard remote forward proxy.
/// This is public trust material only; no session token or secret is read.
/// Returns valid key-ID-specific CA files already cached locally. A missing
/// cache is expected before the first remote run.
pub fn cached_remote_proxy_ca_files() -> Result<Vec<PathBuf>> {
    let directory = remote_proxy_ca_directory()?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = fs::read_dir(&directory)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pem"))
        .collect::<Vec<_>>();
    for path in &files {
        validate_remote_proxy_ca_file(path)?;
    }
    files.sort();
    Ok(files)
}

/// Caches the public CA delivered with a remote forward-proxy session. The
/// backend digest is verified before any file is trusted by a child process.
pub fn provision_remote_proxy_ca(
    certificate: &crate::api::remote_proxy::RemoteProxyCa,
) -> Result<PathBuf> {
    provision_remote_proxy_ca_at(&remote_proxy_ca_directory()?, certificate)
}

fn provision_remote_proxy_ca_at(
    directory: &Path,
    certificate: &crate::api::remote_proxy::RemoteProxyCa,
) -> Result<PathBuf> {
    let actual_sha256 = format!("{:x}", Sha256::digest(certificate.pem.as_bytes()));
    if actual_sha256 != certificate.sha256 {
        anyhow::bail!("Agent Proxy CA digest did not match the session response");
    }
    reqwest::Certificate::from_pem(certificate.pem.as_bytes())
        .context("Agent Proxy session returned an invalid CA PEM")?;

    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "could not create Agent Proxy CA directory at {}",
            directory.display()
        )
    })?;
    #[cfg(unix)]
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;

    let certificate_path = remote_proxy_ca_path(directory, &certificate.key_id)?;
    if validate_remote_proxy_ca_file(&certificate_path).is_ok()
        && fs::read_to_string(&certificate_path)
            .map(|pem| format!("{:x}", Sha256::digest(pem.as_bytes())) == certificate.sha256)
            .unwrap_or(false)
    {
        return Ok(certificate_path);
    }

    write_remote_proxy_ca_file(&certificate_path, certificate.pem.as_bytes())?;
    Ok(certificate_path)
}

fn remote_proxy_ca_path(directory: &Path, key_id: &str) -> Result<PathBuf> {
    if key_id.is_empty()
        || matches!(key_id, "." | "..")
        || !key_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        anyhow::bail!("Agent Proxy session returned an unsafe CA key ID");
    }
    Ok(directory.join(format!("remote-proxy-{key_id}.pem")))
}

fn remote_proxy_ca_directory() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".stashbase/remote-proxy"))
        .context("could not determine the Stashbase Agent Proxy CA path")
}

fn write_remote_proxy_ca_file(path: &Path, contents: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    fs::write(&temporary, contents).with_context(|| {
        format!(
            "could not write Agent Proxy CA cache at {}",
            temporary.display()
        )
    })?;
    #[cfg(unix)]
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(&temporary, path).with_context(|| {
        format!(
            "could not update Agent Proxy CA cache at {}",
            path.display()
        )
    })
}

fn validate_remote_proxy_ca_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        anyhow::bail!(
            "Agent Proxy CA certificate was not found at {}",
            path.display()
        );
    }
    reqwest::Certificate::from_pem(&fs::read(path).with_context(|| {
        format!(
            "could not read Agent Proxy CA certificate at {}",
            path.display()
        )
    })?)
    .with_context(|| {
        format!(
            "Agent Proxy CA certificate at {} is not valid PEM",
            path.display()
        )
    })?;
    Ok(())
}

async fn run_listener(
    listener: TcpListener,
    state: ProxyState,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut revocation_check = tokio::time::interval(Duration::from_millis(50));
    let mut revoked = false;
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            _ = revocation_check.tick(), if !revoked => {
                if state.is_revoked() {
                    state.connections.close_existing();
                    revoked = true;
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let connections = state.connections.clone();
                    let task = tokio::spawn(async move {
                        let service = service_fn(move |request| proxy_request(request, state.clone(), None));
                        let _ = http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .with_upgrades()
                            .await;
                    });
                    connections.track(task);
                }
                Err(_) => break,
            }
        }
    }
}

fn proxy_request(
    mut request: Request<Incoming>,
    state: ProxyState,
    connect_authority: Option<String>,
) -> ProxyFuture {
    Box::pin(async move {
        let started = Instant::now();
        if state.is_revoked() {
            state.record_audit(
                "session_revoked",
                None,
                Some(request.method()),
                None,
                Some(StatusCode::BAD_GATEWAY),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response(
                StatusCode::BAD_GATEWAY,
                "proxy.session_revoked",
                "Agent Proxy session was revoked",
            ));
        }
        if request.method() == Method::CONNECT {
            let authority = request.uri().authority().map(|value| value.to_string());
            let Some(authority) = authority else {
                return Ok(proxy_error_response(
                    StatusCode::BAD_REQUEST,
                    "proxy.invalid_connect",
                    "CONNECT requires an authority",
                ));
            };
            // CONNECT happens before the TLS request headers are available. A
            // secret-only host may therefore open a provisional tunnel, but the
            // intercepted HTTP request still must carry an allowed placeholder
            // unless the host is also ordinary egress.
            if !state.host_allowed_for_connect(Some(host_from_authority(&authority))) {
                debug!(
                    "proxy denied destination: {}",
                    host_from_authority(&authority)
                );
                state.record_audit(
                    "host_denied",
                    Some(host_from_authority(&authority)),
                    Some(&Method::CONNECT),
                    None,
                    Some(StatusCode::FORBIDDEN),
                    Some(started.elapsed()),
                );
                return Ok(proxy_error_response(
                    StatusCode::FORBIDDEN,
                    "proxy.host_not_allowed",
                    "Agent Proxy policy denied destination",
                ));
            }
            let connect_remote = state.remote_for(Some(host_from_authority(&authority)));
            if let Some(remote) = &connect_remote {
                if let Err(error) = remote.token_for_new_connection() {
                    state.record_audit(
                        "session_expired",
                        Some(host_from_authority(&authority)),
                        Some(&Method::CONNECT),
                        None,
                        Some(StatusCode::SERVICE_UNAVAILABLE),
                        Some(started.elapsed()),
                    );
                    return Ok(proxy_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "proxy.session_expired",
                        &error.to_string(),
                    ));
                }
            }
            // Establish the remote CONNECT before acknowledging the child's CONNECT.
            // Otherwise a rejected or stalled remote proxy produces a misleading local
            // 200 response followed by an unexplained dead tunnel.
            let remote_tunnel = match connect_remote
                .clone()
                .filter(|remote| remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept)
            {
                Some(remote) => match establish_remote_connect(&authority, &remote).await {
                    Ok(upstream) => Some(upstream),
                    Err(error) => {
                        debug!("remote proxy CONNECT setup failed: {error:#}");
                        state.record_audit(
                            "remote_connect_failed",
                            Some(host_from_authority(&authority)),
                            Some(&Method::CONNECT),
                            None,
                            Some(StatusCode::BAD_GATEWAY),
                            Some(started.elapsed()),
                        );
                        return Ok(proxy_error_response(
                            StatusCode::BAD_GATEWAY,
                            "proxy.remote_connect_failed",
                            "Unable to establish Agent Proxy tunnel",
                        ));
                    }
                },
                None => None,
            };
            state.record_audit(
                "connect_allowed",
                Some(host_from_authority(&authority)),
                Some(&Method::CONNECT),
                None,
                Some(StatusCode::OK),
                Some(started.elapsed()),
            );
            let connections = state.connections.clone();
            let connection_state = state.clone();
            let task = tokio::spawn(async move {
                match hyper::upgrade::on(&mut request).await {
                    Ok(upgraded) => {
                        if let Some(upstream) = remote_tunnel {
                            if let Err(error) = tunnel_remote_connect(
                                upgraded,
                                authority,
                                upstream,
                                connection_state,
                            )
                            .await
                            {
                                debug!("remote proxy CONNECT tunnel failed: {error:#}");
                            }
                        } else {
                            let _ =
                                serve_tls_connection(upgraded, authority, connection_state).await;
                        }
                    }
                    Err(_) => connection_state.record_audit(
                        "connect_upgrade_failed",
                        Some(host_from_authority(&authority)),
                        Some(&Method::CONNECT),
                        None,
                        None,
                        None,
                    ),
                }
            });
            connections.track(task);
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .body(full_body(Bytes::new()))
                .unwrap());
        }

        if request.uri().path() == DEPENDENCY_HOOK_PATH {
            return Ok(handle_dependency_hook(request, &state).await);
        }
        if let Some(mode) = request
            .uri()
            .path()
            .strip_prefix(SECRET_SCAN_HOOK_PATH)
            .map(str::to_owned)
        {
            return Ok(handle_secret_scan_hook(request, &mode, &state, started).await);
        }

        let request_id = new_local_request_id();
        let host = request_host(&request, connect_authority.as_deref());
        // The agent controls its own environment, so the STASHBASE_SANDBOX
        // marker alone cannot keep telemetry out of an agent session. Every
        // request of a local session passes through this proxy, so refuse the
        // CLI's telemetry POST here, ahead of any egress policy.
        if crate::telemetry::send::is_telemetry_request(
            crate::telemetry::send::destination_host().as_deref(),
            host.as_deref(),
            request.method().as_str(),
            request.uri().path(),
        ) {
            state.record_audit_with_request(
                &request_id,
                "telemetry_blocked",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response_with_id(
                StatusCode::FORBIDDEN,
                "proxy.telemetry_not_allowed",
                "Agent Proxy never forwards Stashbase telemetry from an agent session",
                Some(&request_id),
            ));
        }
        if state.host_is_denied(host.as_deref()) {
            debug!(
                "proxy denied destination: {}",
                host.as_deref().unwrap_or("unknown")
            );
            state.record_audit_with_request(
                &request_id,
                "host_denied",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response_with_id(
                StatusCode::FORBIDDEN,
                "proxy.host_not_allowed",
                "Agent Proxy policy denied destination",
                Some(&request_id),
            ));
        }
        if state.policy.egress_hosts_configured
            && !host
                .as_deref()
                .is_some_and(|host| policy_allows_egress(&state.policy, host))
        {
            state.record_audit_with_request(
                &request_id,
                "host_denied",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response_with_id(
                StatusCode::FORBIDDEN,
                "proxy.host_not_allowed",
                "Agent Proxy policy denied destination",
                Some(&request_id),
            ));
        }
        if contains_unknown_placeholder(&request, &state) {
            state.record_audit_with_request(
                &request_id,
                "unknown_placeholder",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response_with_id(
                StatusCode::FORBIDDEN,
                "proxy.placeholder_not_allowed",
                "Agent Proxy received an unknown credential placeholder",
                Some(&request_id),
            ));
        }
        let secret_name = match replace_placeholder(&mut request, &state, host.as_deref()) {
            Ok(secret_name) => secret_name,
            Err(denial) => {
                debug!(
                    "proxy denied credential injection for destination: {}",
                    host.as_deref().unwrap_or("unknown")
                );
                state.record_audit_with_request(
                    &request_id,
                    denial.audit_action,
                    host.as_deref(),
                    Some(request.method()),
                    Some(&denial.secret_name),
                    Some(StatusCode::FORBIDDEN),
                    Some(started.elapsed()),
                );
                return Ok(proxy_error_response_with_id(
                    StatusCode::FORBIDDEN,
                    "proxy.credential_not_allowed",
                    "The supplied credential is not allowed for this request.",
                    Some(&request_id),
                ));
            }
        };
        // A per-secret host is not general egress. It is permitted only when
        // this request carries that secret's configured placeholder; all other
        // traffic must be explicitly listed in `egress_hosts`.
        if secret_name.is_none() && !state.host_allowed_for_ordinary_request(host.as_deref()) {
            state.record_audit_with_request(
                &request_id,
                "host_denied",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response_with_id(
                StatusCode::FORBIDDEN,
                "proxy.host_not_allowed",
                "Agent Proxy policy denied destination",
                Some(&request_id),
            ));
        }
        let remote = state.remote_for(host.as_deref());
        // Credential routing sends this host direct, so the remote proxy never
        // sees the request and cannot resolve the placeholder. Refuse instead of
        // forwarding a placeholder (or a personal credential reference) upstream.
        if secret_name.is_some() && state.remote.is_some() && remote.is_none() {
            state.record_audit_with_request(
                &request_id,
                "credential_host_not_routed",
                host.as_deref(),
                Some(request.method()),
                secret_name.as_deref(),
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response_with_id(
                StatusCode::FORBIDDEN,
                "proxy.credential_host_not_routed",
                "This destination is not routed through the Agent Proxy, so the credential cannot be applied.",
                Some(&request_id),
            ));
        }
        if let Some(remote) = &remote {
            if let Err(error) = remote.token_for_new_connection() {
                state.record_audit_with_request(
                    &request_id,
                    "session_expired",
                    host.as_deref(),
                    Some(request.method()),
                    secret_name.as_deref(),
                    Some(StatusCode::SERVICE_UNAVAILABLE),
                    Some(started.elapsed()),
                );
                return Ok(proxy_error_response_with_id(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "proxy.session_expired",
                    &error.to_string(),
                    Some(&request_id),
                ));
            }
        }
        // Reqwest deliberately does not support HTTP upgrade responses. Coding agents
        // such as Codex use a WSS connection for streaming, so tunnel an upgraded
        // connection after applying the same destination and placeholder checks.
        if is_upgrade_request(&request) {
            if let Some(remote) = remote.clone() {
                return forward_remote_upgrade(
                    request,
                    state,
                    remote,
                    connect_authority,
                    host,
                    secret_name,
                    started,
                )
                .await;
            }
            return forward_upgrade(
                request,
                state,
                connect_authority,
                host,
                secret_name,
                started,
            )
            .await;
        }
        let url = match request_url(&request, connect_authority.as_deref()) {
            Ok(url) => url,
            Err(_) => {
                state.record_audit_with_request(
                    &request_id,
                    "request_invalid",
                    host.as_deref(),
                    Some(request.method()),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_REQUEST),
                    Some(started.elapsed()),
                );
                return Ok(proxy_error_response_with_id(
                    StatusCode::BAD_REQUEST,
                    "proxy.request_invalid",
                    "Unable to determine request URL",
                    Some(&request_id),
                ));
            }
        };
        let method = request.method().clone();
        let mcp_inspection =
            is_authorized_mcp_inspection(request.headers(), &state.mcp_inspection_token);
        request.headers_mut().remove(MCP_INSPECTION_HEADER);
        let mut headers = request.headers().clone();
        let mcp_path = request.uri().path().to_owned();
        let mcp_rule = matching_mcp_rule(&state.policy, host.as_deref(), &method, &mcp_path);
        let mut mcp_method = None;
        let mut mcp_tool_for_audit = None;
        // `Incoming` is converted into a data stream without collecting it. Reqwest
        // applies chunked transfer encoding when no content length is available, so
        // streaming uploads retain their incremental delivery to the upstream.
        let request_bytes = Arc::new(AtomicU64::new(0));
        let request_byte_counter = request_bytes.clone();
        let body = if mcp_rule.is_some() && method == Method::POST {
            let bytes = match request.into_body().collect().await {
                Ok(body) => body.to_bytes(),
                Err(_) => {
                    return Ok(proxy_error_response_with_id(
                        StatusCode::BAD_REQUEST,
                        "proxy.mcp_request_invalid",
                        "Unable to read MCP request body",
                        Some(&request_id),
                    ));
                }
            };
            let mcp_tool = mcp_tool_name(&bytes);
            if let Err(message) =
                authorize_mcp_request(&bytes, &state.policy, host.as_deref(), &mcp_path)
            {
                if let Some(tool) = mcp_tool.as_deref() {
                    state.record_mcp_tool_with_request(
                        &request_id,
                        "mcp_tool_denied",
                        tool,
                        host.as_deref(),
                        Some(&mcp_path),
                        Some(&method),
                        None,
                        Some(StatusCode::FORBIDDEN),
                        Some(started.elapsed()),
                    );
                } else {
                    state.record_audit_with_request(
                        &request_id,
                        "mcp_tool_denied",
                        host.as_deref(),
                        Some(&method),
                        None,
                        Some(StatusCode::FORBIDDEN),
                        Some(started.elapsed()),
                    );
                }
                return Ok(proxy_error_response_with_id(
                    StatusCode::FORBIDDEN,
                    "proxy.mcp_tool_not_allowed",
                    message,
                    Some(&request_id),
                ));
            }
            mcp_method = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("method")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                });
            if mcp_method.as_deref() == Some("tools/call") {
                mcp_tool_for_audit = mcp_tool;
            }
            request_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            reqwest::Body::from(bytes)
        } else {
            reqwest::Body::wrap_stream(request.into_body().into_data_stream().map(move |chunk| {
                if let Ok(bytes) = &chunk {
                    request_byte_counter.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                }
                chunk
            }))
        };

        let destination_url = if let Some(remote) = remote
            .as_ref()
            .filter(|remote| remote.protocol == RemoteProxyProtocol::Custom)
        {
            headers.remove("x-stashbase-target");
            headers.remove("x-stashbase-session");
            let token = match remote.token_for_new_connection() {
                Ok(token) => token,
                Err(error) => {
                    return Ok(proxy_error_response_with_id(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "proxy.session_expired",
                        &error.to_string(),
                        Some(&request_id),
                    ))
                }
            };
            headers.insert(
                "x-stashbase-target",
                HeaderValue::from_str(url.as_str()).unwrap(),
            );
            let token = match HeaderValue::from_str(&token) {
                Ok(token) => token,
                Err(_) => {
                    return Ok(proxy_error_response_with_id(
                        StatusCode::BAD_GATEWAY,
                        "proxy.session_invalid",
                        "Agent Proxy returned an invalid session token",
                        Some(&request_id),
                    ))
                }
            };
            headers.insert("x-stashbase-session", token);
            let proxy = match reqwest::Url::parse(&remote.proxy_url) {
                Ok(proxy) => proxy,
                Err(_) => {
                    return Ok(proxy_error_response_with_id(
                        StatusCode::BAD_GATEWAY,
                        "proxy.session_invalid",
                        "Agent Proxy returned an invalid proxy URL",
                        Some(&request_id),
                    ))
                }
            };
            // This is a forward request to the remote proxy, not the original
            // destination. Keeping the child's Host header sends the wrong virtual
            // host for ordinary (non-upgrade) requests.
            let proxy_host = match proxy_host_header(&proxy) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(proxy_error_response_with_id(
                        StatusCode::BAD_GATEWAY,
                        "proxy.session_invalid",
                        "Agent Proxy returned an invalid proxy URL",
                        Some(&request_id),
                    ))
                }
            };
            headers.insert(hyper::header::HOST, proxy_host);
            remote.proxy_url.clone()
        } else {
            url.to_string()
        };
        let client = match remote
            .as_ref()
            .filter(|remote| remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept)
        {
            Some(remote) => match remote_forward_client(remote, state.remote_ca.as_ref()) {
                Ok(client) => client,
                Err(error) => {
                    return Ok(proxy_error_response_with_id(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "proxy.session_unavailable",
                        &error.to_string(),
                        Some(&request_id),
                    ))
                }
            },
            None => state.client.clone(),
        };
        match client
            .request(method.clone(), destination_url)
            .headers(headers)
            .body(body)
            .send()
            .await
        {
            Ok(upstream) => {
                let status = upstream.status();
                let mut headers = upstream.headers().clone();
                let content_type = headers
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_ascii_lowercase);
                let filter_mcp_list = mcp_method.as_deref() == Some("tools/list")
                    && mcp_rule.is_some()
                    && !mcp_inspection;
                let mcp_json_response = filter_mcp_list
                    && content_type
                        .as_deref()
                        .is_some_and(|value| value.contains("application/json"));
                let mcp_sse_response = filter_mcp_list
                    && content_type
                        .as_deref()
                        .is_some_and(|value| value.contains("text/event-stream"));
                if mcp_json_response {
                    let response_bytes = match upstream.bytes().await {
                        Ok(bytes) => filter_mcp_tools_list_response(
                            &bytes,
                            &state.policy,
                            host.as_deref(),
                            &mcp_path,
                        )
                        .unwrap_or(bytes),
                        Err(error) => {
                            return Ok(proxy_error_response_with_id(
                                StatusCode::BAD_GATEWAY,
                                "proxy.mcp_response_invalid",
                                &format!("Unable to read MCP tools/list response: {error}"),
                                Some(&request_id),
                            ));
                        }
                    };
                    request_bytes.fetch_add(response_bytes.len() as u64, Ordering::Relaxed);
                    headers.remove("content-length");
                    let mut response = Response::builder()
                        .status(status)
                        .body(full_body(response_bytes))
                        .unwrap();
                    *response.headers_mut() = headers;
                    return Ok(response);
                }
                if mcp_sse_response {
                    headers.remove("content-length");
                    let action = if secret_name.is_some() {
                        "injected"
                    } else {
                        "forwarded"
                    };
                    let filtered = McpToolsListSseStream::new(
                        upstream.bytes_stream(),
                        state.policy.clone(),
                        host.clone(),
                        mcp_path.clone(),
                    );
                    let body = StreamBody::new(AuditedResponseStream::new(
                        filtered,
                        request_bytes,
                        state.audit_log.clone(),
                        request_id,
                        action,
                        host,
                        None,
                        method,
                        secret_name,
                        None,
                        status,
                        started,
                    ))
                    .boxed_unsync();
                    let mut response = Response::builder().status(status).body(body).unwrap();
                    *response.headers_mut() = headers;
                    return Ok(response);
                }
                // Do not await `bytes()`: forwarding this stream lets clients observe
                // each upstream chunk (including SSE events) as it arrives.
                let action = if secret_name.is_some() {
                    "injected"
                } else {
                    "forwarded"
                };
                let audit_action = if mcp_tool_for_audit.is_some() {
                    "mcp_tool_call"
                } else {
                    action
                };
                let body = StreamBody::new(AuditedResponseStream::new(
                    upstream.bytes_stream(),
                    request_bytes,
                    state.audit_log.clone(),
                    request_id,
                    audit_action,
                    host.clone(),
                    mcp_tool_for_audit.as_ref().map(|_| mcp_path.clone()),
                    method.clone(),
                    secret_name.clone(),
                    mcp_tool_for_audit,
                    status,
                    started,
                ))
                .boxed_unsync();
                let mut response = Response::builder().status(status).body(body).unwrap();
                *response.headers_mut() = headers;
                Ok(response)
            }
            Err(error) => {
                debug!(
                    "proxy could not forward request to destination: {}",
                    host.as_deref().unwrap_or("unknown")
                );
                state.record_audit_with_request(
                    &request_id,
                    upstream_error_action(&error),
                    host.as_deref(),
                    Some(&method),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_GATEWAY),
                    Some(started.elapsed()),
                );
                Ok(proxy_error_response_with_id(
                    StatusCode::BAD_GATEWAY,
                    &format!("proxy.{}", upstream_error_action(&error)),
                    "Unable to forward Agent Proxy request",
                    Some(&request_id),
                ))
            }
        }
    })
}

async fn handle_dependency_hook(
    request: Request<Incoming>,
    state: &ProxyState,
) -> Response<ProxyBody> {
    let Some(broker) = state
        .hook_broker
        .as_ref()
        .filter(|broker| broker.dependency_check && broker.authorized(&request))
    else {
        return proxy_error_response(
            StatusCode::FORBIDDEN,
            "proxy.dependency_hook_not_allowed",
            "Dependency hook route is not enabled for this run",
        );
    };
    let client = &broker.client;
    let body = match request.into_body().collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => {
            return proxy_error_response(
                StatusCode::BAD_REQUEST,
                "proxy.invalid_body",
                "Invalid dependency hook request",
            )
        }
    };
    let response = match client
        .post(format!(
            "{}/v1/dependencies/check",
            crate::api::client::get_api_url()
        ))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return proxy_error_response(
                StatusCode::BAD_GATEWAY,
                "proxy.dependency_hook_failed",
                "Dependency check request failed",
            )
        }
    };
    let status = response.status();
    let content_type = response.headers().get("content-type").cloned();
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(_) => {
            return proxy_error_response(
                StatusCode::BAD_GATEWAY,
                "proxy.dependency_hook_failed",
                "Dependency check response failed",
            )
        }
    };
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(full_body(body)).unwrap()
}

/// Keeps at most `limit` bytes of `reader`, discarding the rest as it
/// arrives. The scan's output size is agent-controlled, and draining (rather
/// than stopping) keeps the child from blocking on a full pipe.
async fn read_capped(mut reader: impl tokio::io::AsyncRead + Unpin, limit: usize) -> Vec<u8> {
    use tokio::io::AsyncReadExt;

    let mut kept = Vec::new();
    let _ = (&mut reader)
        .take(limit as u64)
        .read_to_end(&mut kept)
        .await;
    let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
    kept
}

/// Runs the host's own `stashbase scan` for a sandboxed git hook. The hook
/// sends only the mode; what gets scanned, with which key and which config
/// options, is decided here.
async fn handle_secret_scan_hook(
    request: Request<Incoming>,
    mode: &str,
    state: &ProxyState,
    started: Instant,
) -> Response<ProxyBody> {
    let request_id = new_local_request_id();
    let Some((broker, scan)) = state
        .hook_broker
        .as_ref()
        .filter(|broker| broker.authorized(&request))
        .and_then(|broker| broker.secret_scan.as_ref().map(|scan| (broker, scan)))
    else {
        return proxy_error_response(
            StatusCode::FORBIDDEN,
            "proxy.secret_scan_not_allowed",
            "Secret scan hook is not enabled for this run; add \"secret_scan\" to allow_hooks",
        );
    };
    let mode = match mode {
        "/staged" => "staged",
        "/unpushed" => "unpushed",
        _ => {
            return proxy_error_response(
                StatusCode::NOT_FOUND,
                "proxy.secret_scan_unknown_mode",
                "Unknown scan mode; expected staged or unpushed",
            )
        }
    };

    let _guard = broker.scan_lock.lock().await;
    let scan_failed = || {
        proxy_error_response(
            StatusCode::BAD_GATEWAY,
            "proxy.secret_scan_failed",
            "Secret scan could not be started",
        )
    };
    let Ok(home) = ScratchHome::create() else {
        return scan_failed();
    };
    let scan_args = ["scan", mode, "--json", "--silent"];
    let (program, args) = match &scan.isolation {
        ScanIsolation::Confined(confinement) => match confinement.wrap(&scan_args, &home.0) {
            Ok(command) => command,
            Err(_) => return scan_failed(),
        },
        #[cfg(test)]
        ScanIsolation::Unconfined { exe } => (
            exe.to_string_lossy().into_owned(),
            scan_args.iter().map(|arg| (*arg).to_owned()).collect(),
        ),
    };
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(&scan.workdir)
        .env("HOME", &home.0)
        // The API this run uses, whether it came from the environment or was
        // built in, so the scan never falls back to a different default.
        .env(
            crate::api::client::API_URL_ENV_VAR,
            crate::api::client::get_api_url(),
        )
        .env("TMPDIR", &home.0)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_STATE_HOME")
        .env("STASHBASE_API_KEY", &broker.api_key)
        .env(crate::models::scans::SCAN_RESTRICTED_ENV, "1")
        // Keeps the scan's telemetry off, like everything else in the session.
        .env("STASHBASE_SANDBOX", "1")
        .env_remove(crate::api::dependencies::HOOK_MODE_ENV)
        .env_remove(crate::api::dependencies::HOOK_BROKER_URL_ENV)
        .env_remove(crate::api::dependencies::HOOK_BROKER_TOKEN_ENV)
        .env_remove(SCAN_BROKER_URL_ENV)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let run = async move {
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let (stdout, stderr, status) = tokio::join!(
            read_capped(stdout, SECRET_SCAN_BODY_LIMIT),
            read_capped(stderr, SECRET_SCAN_BODY_LIMIT),
            child.wait(),
        );
        status.map(|status| (status, stdout, stderr))
    };
    // On timeout `run` is dropped with the child, which `kill_on_drop` ends.
    let response = match tokio::time::timeout(scan.timeout, run).await {
        Err(_) => proxy_error_response(
            StatusCode::GATEWAY_TIMEOUT,
            "proxy.secret_scan_timeout",
            "Secret scan did not finish in time",
        ),
        Ok(Err(_)) => proxy_error_response(
            StatusCode::BAD_GATEWAY,
            "proxy.secret_scan_failed",
            "Secret scan could not be started",
        ),
        Ok(Ok((exit, stdout, stderr))) => {
            let status = match exit.code() {
                Some(0) => StatusCode::OK,
                Some(1) => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::BAD_GATEWAY,
            };
            let mut body = stdout;
            body.extend_from_slice(&stderr);
            if status == StatusCode::BAD_GATEWAY {
                // A scan killed before printing anything (e.g. by the
                // confinement) would otherwise leave an empty body.
                body.extend_from_slice(format!("\nstashbase scan exited with {exit}\n").as_bytes());
            }
            body.truncate(SECRET_SCAN_BODY_LIMIT);
            Response::builder()
                .status(status)
                .header("content-type", "text/plain; charset=utf-8")
                .body(full_body(Bytes::from(body)))
                .unwrap()
        }
    };
    state.record_audit_with_request(
        &request_id,
        "secret_scan_hook",
        None,
        Some(&Method::POST),
        None,
        Some(response.status()),
        Some(started.elapsed()),
    );
    response
}

/// Standard remote-proxy requests are built per request so a new connection
/// always observes the latest rotated token. Existing response streams retain
/// the client and session that opened them.
fn remote_forward_client(
    remote: &RemoteProxyConfig,
    remote_ca: Option<&reqwest::Certificate>,
) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(
            REQUEST_TIMEOUT_SECS.get().copied().unwrap_or(30),
        ))
        .read_timeout(Duration::from_secs(
            REQUEST_TIMEOUT_SECS.get().copied().unwrap_or(30),
        ))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(certificate) = remote_ca {
        builder = builder.add_root_certificate(certificate.clone());
    }
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::PROXY_AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", remote.token_for_new_connection()?))?,
    );
    Ok(builder
        .proxy(reqwest::Proxy::all(&remote.proxy_url)?.headers(headers))
        .build()?)
}

/// Establishes the remote side of a CONNECT tunnel before the child is told
/// that its local CONNECT succeeded. The session token stays in this handshake
/// and is never placed in the child environment.
async fn establish_remote_connect(
    authority: &str,
    remote: &RemoteProxyConfig,
) -> Result<Box<dyn AsyncStream>> {
    let token = remote.token_for_new_connection()?;
    let proxy = reqwest::Url::parse(&remote.proxy_url).context("invalid remote proxy URL")?;
    let timeout = Duration::from_secs(REQUEST_TIMEOUT_SECS.get().copied().unwrap_or(30));
    let mut upstream = tokio::time::timeout(timeout, connect_remote_proxy(&proxy))
        .await
        .context("Agent Proxy CONNECT setup timed out")??;
    tokio::time::timeout(timeout, async {
        upstream
            .write_all(
            format!(
                "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Authorization: Bearer {}\r\n\r\n",
                token
            )
            .as_bytes(),
            )
            .await?;
        read_connect_response(&mut upstream).await
    })
    .await
    .context("Agent Proxy CONNECT handshake timed out")??;
    Ok(upstream)
}

async fn read_connect_response(upstream: &mut (dyn AsyncStream + 'static)) -> Result<()> {
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while response.len() < 16 * 1024 {
        upstream.read_exact(&mut byte).await?;
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    if !response.starts_with(b"HTTP/1.1 200") && !response.starts_with(b"HTTP/1.0 200") {
        anyhow::bail!("Agent Proxy rejected CONNECT");
    }
    Ok(())
}

/// Bridges the already-established remote tunnel to the local child tunnel.
async fn tunnel_remote_connect(
    upgraded: hyper::upgrade::Upgraded,
    authority: String,
    mut upstream: Box<dyn AsyncStream>,
    state: ProxyState,
) -> Result<()> {
    let mut child = TokioIo::new(upgraded);
    let _ = copy_bidirectional(&mut child, &mut upstream).await;
    state.record_audit(
        "remote_connect_closed",
        Some(host_from_authority(&authority)),
        Some(&Method::CONNECT),
        None,
        None,
        None,
    );
    Ok(())
}

trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T> AsyncStream for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

/// Opens the connection to the remote proxy itself. Standard forward proxies
/// can be served over either HTTP or HTTPS; CONNECT must use TLS for the latter
/// before its plaintext HTTP handshake is written.
async fn connect_remote_proxy(proxy: &reqwest::Url) -> Result<Box<dyn AsyncStream>> {
    let host = proxy.host_str().context("remote proxy URL has no host")?;
    let port = proxy
        .port_or_known_default()
        .context("remote proxy URL has no known port")?;
    let stream = TcpStream::connect(format!("{host}:{port}")).await?;
    match proxy.scheme() {
        "http" => Ok(Box::new(stream)),
        "https" => {
            let server_name = ServerName::try_from(host.to_owned())
                .context("remote proxy URL has an invalid TLS host")?;
            let config = ClientConfig::builder()
                .with_platform_verifier()?
                .with_no_client_auth();
            Ok(Box::new(
                TlsConnector::from(Arc::new(config))
                    .connect(server_name, stream)
                    .await?,
            ))
        }
        scheme => anyhow::bail!("remote proxy URL uses unsupported scheme: {scheme}"),
    }
}

/// Sends an intercepted WebSocket opening handshake to the remote proxy. The
/// remote service owns placeholder resolution; this relay retains only the
/// opaque session token and streams frames after the HTTP/1 upgrade.
async fn forward_remote_upgrade(
    mut request: Request<Incoming>,
    state: ProxyState,
    remote: RemoteProxyConfig,
    connect_authority: Option<String>,
    host: Option<String>,
    secret_name: Option<String>,
    started: Instant,
) -> Result<Response<ProxyBody>, Infallible> {
    // ForwardProxyTlsIntercept: the remote proxy handles TLS interception for
    // HTTPS. For plain-HTTP upgrades (ws://) the correct path is also a CONNECT
    // tunnel so the remote proxy can apply its own policy. Sending the upgrade
    // handshake directly to the proxy URL would bypass the CONNECT protocol and
    // be rejected by a standard forward proxy.
    if remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept {
        let authority = match upstream_authority(&request, connect_authority.as_deref()) {
            Ok(authority) => authority,
            Err(_) => {
                return Ok(proxy_error_response(
                    StatusCode::BAD_REQUEST,
                    "proxy.request_invalid",
                    "Unable to determine request URL",
                ));
            }
        };
        let upstream = match establish_remote_connect(&authority, &remote).await {
            Ok(upstream) => upstream,
            Err(error) => {
                debug!("remote proxy upgrade CONNECT setup failed: {error:#}");
                return Ok(upgrade_error_response(
                    &state,
                    host.as_deref(),
                    secret_name.as_deref(),
                    started,
                ));
            }
        };
        // A child speaks absolute-form to this local proxy, but after CONNECT
        // the remote side is an origin connection and requires origin-form.
        *request.uri_mut() = upgrade_origin_form_uri(&request);
        return tunnel_upgrade(request, upstream, state, host, secret_name, started).await;
    }

    let target = match request_url(&request, connect_authority.as_deref()) {
        Ok(target) => target
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1),
        Err(_) => {
            return Ok(proxy_error_response(
                StatusCode::BAD_REQUEST,
                "proxy.request_invalid",
                "Unable to determine request URL",
            ))
        }
    };
    let proxy = match reqwest::Url::parse(&remote.proxy_url) {
        Ok(proxy) => proxy,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ))
        }
    };
    let Some(proxy_host) = proxy.host_str().map(str::to_owned) else {
        return Ok(upgrade_error_response(
            &state,
            host.as_deref(),
            secret_name.as_deref(),
            started,
        ));
    };
    let Some(port) = proxy.port_or_known_default() else {
        return Ok(upgrade_error_response(
            &state,
            host.as_deref(),
            secret_name.as_deref(),
            started,
        ));
    };
    let stream = match TcpStream::connect(format!("{proxy_host}:{port}")).await {
        Ok(stream) => stream,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ))
        }
    };
    request.headers_mut().remove("x-stashbase-target");
    request.headers_mut().remove("x-stashbase-session");
    request.headers_mut().insert(
        "x-stashbase-target",
        HeaderValue::from_str(&target).unwrap(),
    );
    let token = match remote.token_for_new_connection() {
        Ok(token) => token,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ))
        }
    };
    let token = match HeaderValue::from_str(&token) {
        Ok(token) => token,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ))
        }
    };
    request.headers_mut().insert("x-stashbase-session", token);
    let proxy_host_header = match proxy_host_header(&proxy) {
        Ok(value) => value,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ))
        }
    };
    request
        .headers_mut()
        .insert(hyper::header::HOST, proxy_host_header);
    let proxy_path = match proxy.query() {
        Some(query) => format!("{}?{query}", proxy.path()),
        None => proxy.path().to_owned(),
    };
    *request.uri_mut() = proxy_path.parse().unwrap();
    if proxy.scheme() == "https" {
        let server_name = match ServerName::try_from(proxy_host.clone()) {
            Ok(value) => value,
            Err(_) => {
                return Ok(upgrade_error_response(
                    &state,
                    host.as_deref(),
                    secret_name.as_deref(),
                    started,
                ))
            }
        };
        let config = match ClientConfig::builder().with_platform_verifier() {
            Ok(value) => value.with_no_client_auth(),
            Err(_) => {
                return Ok(upgrade_error_response(
                    &state,
                    host.as_deref(),
                    secret_name.as_deref(),
                    started,
                ))
            }
        };
        let stream = match TlsConnector::from(Arc::new(config))
            .connect(server_name, stream)
            .await
        {
            Ok(value) => value,
            Err(_) => {
                return Ok(upgrade_error_response(
                    &state,
                    host.as_deref(),
                    secret_name.as_deref(),
                    started,
                ))
            }
        };
        return tunnel_upgrade(request, stream, state, host, secret_name, started).await;
    }
    tunnel_upgrade(request, stream, state, host, secret_name, started).await
}

fn is_upgrade_request(request: &Request<Incoming>) -> bool {
    request.headers().contains_key(hyper::header::UPGRADE)
}

/// For WebSockets and other HTTP/1 upgrades, make the upstream connection with
/// Hyper rather than Reqwest, then copy the two upgraded byte streams. The
/// request has already passed policy checks and placeholder replacement.
async fn forward_upgrade(
    request: Request<Incoming>,
    state: ProxyState,
    connect_authority: Option<String>,
    host: Option<String>,
    secret_name: Option<String>,
    started: Instant,
) -> Result<Response<ProxyBody>, Infallible> {
    let authority = match upstream_authority(&request, connect_authority.as_deref()) {
        Ok(authority) => authority,
        Err(_) => {
            return Ok(proxy_error_response(
                StatusCode::BAD_REQUEST,
                "proxy.request_invalid",
                "Unable to determine request URL",
            ));
        }
    };
    let (hostname, port) = match split_authority(&authority, connect_authority.is_some()) {
        Some(parts) => parts,
        None => {
            return Ok(proxy_error_response(
                StatusCode::BAD_REQUEST,
                "proxy.request_invalid",
                "Unable to determine request URL",
            ));
        }
    };
    let stream = match TcpStream::connect(format!("{hostname}:{port}")).await {
        Ok(stream) => stream,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ));
        }
    };

    if connect_authority.is_some() {
        let server_name = match ServerName::try_from(hostname.clone()) {
            Ok(name) => name,
            Err(_) => {
                return Ok(proxy_error_response(
                    StatusCode::BAD_REQUEST,
                    "proxy.request_invalid",
                    "Unable to determine request URL",
                ));
            }
        };
        let config = match ClientConfig::builder().with_platform_verifier() {
            Ok(config) => config.with_no_client_auth(),
            Err(_) => {
                return Ok(upgrade_error_response(
                    &state,
                    host.as_deref(),
                    secret_name.as_deref(),
                    started,
                ));
            }
        };
        let stream = match TlsConnector::from(Arc::new(config))
            .connect(server_name, stream)
            .await
        {
            Ok(stream) => stream,
            Err(_) => {
                return Ok(upgrade_error_response(
                    &state,
                    host.as_deref(),
                    secret_name.as_deref(),
                    started,
                ));
            }
        };
        return tunnel_upgrade(request, stream, state, host, secret_name, started).await;
    }

    tunnel_upgrade(request, stream, state, host, secret_name, started).await
}

async fn tunnel_upgrade<S>(
    mut request: Request<Incoming>,
    stream: S,
    state: ProxyState,
    host: Option<String>,
    secret_name: Option<String>,
    started: Instant,
) -> Result<Response<ProxyBody>, Infallible>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // `Proxy-Connection` is meaningful only between a client and its proxy.
    request.headers_mut().remove("proxy-connection");
    let client_upgrade = hyper::upgrade::on(&mut request);
    let (mut sender, connection) = match client_http1::handshake(TokioIo::new(stream)).await {
        Ok(connection) => connection,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ));
        }
    };
    let connections = state.connections.clone();
    connections.track(tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    }));

    let mut upstream = match sender.send_request(request).await {
        Ok(response) => response,
        Err(_) => {
            return Ok(upgrade_error_response(
                &state,
                host.as_deref(),
                secret_name.as_deref(),
                started,
            ));
        }
    };
    let status = upstream.status();
    let headers = upstream.headers().clone();
    if status != StatusCode::SWITCHING_PROTOCOLS {
        state.record_audit(
            "upgrade_rejected",
            host.as_deref(),
            None,
            secret_name.as_deref(),
            Some(status),
            Some(started.elapsed()),
        );
        let body = StreamBody::new(upstream.into_body().into_data_stream().map(|chunk| {
            chunk
                .map(Frame::data)
                .map_err(|error| -> BoxError { Box::new(error) })
        }))
        .boxed_unsync();
        let mut response = Response::builder().status(status).body(body).unwrap();
        *response.headers_mut() = headers;
        return Ok(response);
    }

    let upstream_upgrade = hyper::upgrade::on(&mut upstream);
    let state_for_task = state.clone();
    let task = tokio::spawn(async move {
        let Ok(client) = client_upgrade.await else {
            return;
        };
        let Ok(upstream) = upstream_upgrade.await else {
            return;
        };
        let mut client = TokioIo::new(client);
        let mut upstream = TokioIo::new(upstream);
        let _ = copy_bidirectional(&mut client, &mut upstream).await;
        state_for_task.record_audit("upgrade_closed", None, None, None, None, None);
    });
    state.connections.track(task);
    state.record_audit(
        if secret_name.is_some() {
            "injected_upgrade"
        } else {
            "upgrade_tunneled"
        },
        host.as_deref(),
        None,
        secret_name.as_deref(),
        Some(status),
        Some(started.elapsed()),
    );
    let mut response = Response::builder()
        .status(status)
        .body(full_body(Bytes::new()))
        .unwrap();
    *response.headers_mut() = headers;
    Ok(response)
}

fn upgrade_error_response(
    state: &ProxyState,
    host: Option<&str>,
    secret_name: Option<&str>,
    started: Instant,
) -> Response<ProxyBody> {
    state.record_audit(
        "upgrade_failed",
        host,
        None,
        secret_name,
        Some(StatusCode::BAD_GATEWAY),
        Some(started.elapsed()),
    );
    proxy_error_response(
        StatusCode::BAD_GATEWAY,
        "proxy.upgrade_failed",
        "Unable to establish upgraded Agent Proxy connection",
    )
}

fn upstream_authority(
    request: &Request<Incoming>,
    connect_authority: Option<&str>,
) -> Result<String> {
    if let Some(authority) = connect_authority {
        return Ok(authority.to_owned());
    }
    if let Some(authority) = request.uri().authority() {
        return Ok(authority.to_string());
    }
    request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .context("HTTP proxy request is missing a Host header")
}

fn upgrade_origin_form_uri<B>(request: &Request<B>) -> hyper::Uri {
    request
        .uri()
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str())
        .unwrap_or("/")
        .parse()
        .expect("a request URI path and query are always valid URI references")
}

/// Returns the authority understood by the remote proxy's HTTP listener.
/// Explicit non-default ports are part of the Host header's authority.
fn proxy_host_header(proxy: &reqwest::Url) -> Result<HeaderValue> {
    let host = proxy.host_str().context("remote proxy URL has no host")?;
    let authority = match proxy.port() {
        Some(port) if host.contains(':') && !host.starts_with('[') => {
            format!("[{host}]:{port}")
        }
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    HeaderValue::from_str(&authority).context("remote proxy URL has an invalid Host header")
}

fn split_authority(authority: &str, tls: bool) -> Option<(String, u16)> {
    if authority.starts_with('[') {
        let (host, port) = authority.rsplit_once("]:")?;
        return port
            .parse()
            .ok()
            .map(|port| (host.trim_start_matches('[').to_owned(), port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if let Ok(port) = port.parse() {
            return Some((host.to_owned(), port));
        }
    }
    Some((authority.to_owned(), if tls { 443 } else { 80 }))
}

fn request_url(request: &Request<Incoming>, connect_authority: Option<&str>) -> Result<String> {
    if let Some(authority) = connect_authority {
        let path = request
            .uri()
            .path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/");
        return Ok(format!("https://{authority}{path}"));
    }
    if request.uri().scheme().is_some() {
        return Ok(request.uri().to_string());
    }
    let host = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .context("HTTP proxy request is missing a Host header")?;
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    Ok(format!("http://{host}{path}"))
}

async fn serve_tls_connection(
    upgraded: hyper::upgrade::Upgraded,
    authority: String,
    state: ProxyState,
) -> Result<()> {
    let host = authority.split(':').next().unwrap_or(&authority);
    let mut params = CertificateParams::new(vec![host.to_owned()])?;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let leaf_key = KeyPair::generate()?;
    let leaf = params.signed_by(&leaf_key, &state.certificate_authority)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )?;
    let handshake_started = Instant::now();
    let stream = match TlsAcceptor::from(Arc::new(config))
        .accept(TokioIo::new(upgraded))
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            // A client that rejects the temporary CA usually terminates here. The
            // TLS protocol does not reveal the exact client-side reason, so this
            // is intentionally phrased as a trust/handshake failure.
            state.record_audit(
                "tls_trust_failed",
                Some(host),
                Some(&Method::CONNECT),
                None,
                None,
                Some(handshake_started.elapsed()),
            );
            return Err(error.into());
        }
    };
    let service =
        service_fn(move |request| proxy_request(request, state.clone(), Some(authority.clone())));
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        // A CONNECT tunnel can contain a WebSocket upgrade (Codex uses WSS for
        // streaming), so preserve HTTP/1 upgrade support after TLS interception.
        .with_upgrades()
        .await
        .context("TLS proxy connection ended before the HTTP request completed")?;
    Ok(())
}

impl ProxyState {
    fn is_revoked(&self) -> bool {
        self.revocation_path
            .read()
            .ok()
            .and_then(|path| path.clone())
            .is_some_and(|path| crate::handlers::agent::sessions::is_local_session_revoked(&path))
    }

    fn host_is_denied(&self, host: Option<&str>) -> bool {
        host.is_some_and(|host| policy_denies_host(&self.policy, host))
    }

    fn host_allowed_for_connect(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return !self.policy.strict_deny;
        };
        !policy_denies_host(&self.policy, host)
            && (!self.policy.egress_hosts_configured || policy_allows_egress(&self.policy, host))
            && (!self.policy.strict_deny || policy_allows_connect(&self.policy, host))
    }

    fn host_allowed_for_ordinary_request(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return !self.policy.strict_deny;
        };
        !policy_denies_host(&self.policy, host)
            && (!self.policy.strict_deny || policy_allows_egress(&self.policy, host))
    }

    fn record_audit(
        &self,
        action: &str,
        host: Option<&str>,
        method: Option<&Method>,
        secret_name: Option<&str>,
        status: Option<StatusCode>,
        duration: Option<Duration>,
    ) {
        if let Some(audit_log) = &self.audit_log {
            audit_log.record(action, host, method, secret_name, status, duration, None);
        }
    }

    fn record_audit_with_request(
        &self,
        request_id: &str,
        action: &str,
        host: Option<&str>,
        method: Option<&Method>,
        secret_name: Option<&str>,
        status: Option<StatusCode>,
        duration: Option<Duration>,
    ) {
        if let Some(audit_log) = &self.audit_log {
            audit_log.record(
                action,
                host,
                method,
                secret_name,
                status,
                duration,
                Some(request_id),
            );
        }
    }

    fn record_mcp_tool_with_request(
        &self,
        request_id: &str,
        action: &str,
        tool: &str,
        host: Option<&str>,
        path: Option<&str>,
        method: Option<&Method>,
        secret_name: Option<&str>,
        status: Option<StatusCode>,
        duration: Option<Duration>,
    ) {
        if let Some(audit_log) = &self.audit_log {
            audit_log.record_with_bytes(
                action,
                host,
                method,
                secret_name,
                status,
                duration,
                Some(request_id),
                None,
                None,
                path,
                None,
                Some(tool),
            );
        }
    }
}

fn policy_allows_connect(policy: &ProxyPolicy, host: &str) -> bool {
    policy
        .secret_policies
        .values()
        .any(|secret_policy| match secret_policy {
            SecretHttpPolicy::LegacyHosts(hosts) => {
                hosts.iter().any(|allowed| host_matches(allowed, host))
            }
            SecretHttpPolicy::Rules(rules) => rules
                .iter()
                .any(|rule| rule.hosts.iter().any(|allowed| host_matches(allowed, host))),
        })
        || policy_allows_egress(policy, host)
}

fn policy_allows_egress(policy: &ProxyPolicy, host: &str) -> bool {
    policy
        .allowed_egress_hosts
        .iter()
        .any(|allowed| allowed == "*" || host_matches(allowed, host))
}

fn matching_mcp_rule<'a>(
    policy: &'a ProxyPolicy,
    host: Option<&str>,
    method: &Method,
    path: &str,
) -> Option<&'a AgentMcpRule> {
    let host = host?;
    policy.mcp_rules.iter().find(|rule| {
        rule.hosts.iter().any(|allowed| host_matches(allowed, host))
            && rule.paths.iter().any(|pattern| path_matches(pattern, path))
            && (method == Method::POST || method == Method::GET)
    })
}

fn is_authorized_mcp_inspection(headers: &HeaderMap, expected_token: &str) -> bool {
    headers
        .get(MCP_INSPECTION_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected_token)
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim();
    match pattern.strip_suffix('*') {
        Some(prefix) => path.starts_with(prefix),
        None => pattern == path,
    }
}

fn mcp_tool_name(bytes: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    (value.get("method")?.as_str()? == "tools/call")
        .then(|| value.pointer("/params/name")?.as_str().map(str::to_owned))
        .flatten()
}

fn authorize_mcp_request(
    body: &[u8],
    policy: &ProxyPolicy,
    host: Option<&str>,
    path: &str,
) -> std::result::Result<(), &'static str> {
    if !policy.mcp_rules.iter().any(|candidate| {
        candidate
            .hosts
            .iter()
            .any(|allowed| host.is_some_and(|host| host_matches(allowed, host)))
            && candidate
                .paths
                .iter()
                .any(|pattern| path_matches(pattern, path))
    }) {
        return Ok(());
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "MCP request body is not valid JSON")?;
    let Some(method) = value.get("method").and_then(serde_json::Value::as_str) else {
        // JSON-RPC responses to server-initiated requests have no method. They
        // must pass through so MCP capabilities such as sampling and
        // elicitation can complete.
        return Ok(());
    };
    if method != "tools/call" {
        return Ok(());
    }
    let Some(tool) = value
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(serde_json::Value::as_str)
    else {
        return Err("MCP tools/call is missing params.name");
    };
    let allowed = policy.mcp_rules.iter().any(|candidate| {
        candidate.effect == AgentHttpRuleEffect::Allow
            && candidate
                .hosts
                .iter()
                .any(|allowed| host.is_some_and(|host| host_matches(allowed, host)))
            && candidate
                .paths
                .iter()
                .any(|pattern| path_matches(pattern, path))
            && candidate
                .tools
                .iter()
                .any(|candidate_tool| candidate_tool == "*" || candidate_tool == tool)
    });
    let denied = policy.mcp_rules.iter().any(|candidate| {
        candidate.effect == AgentHttpRuleEffect::Deny
            && host.is_some_and(|host| {
                candidate
                    .hosts
                    .iter()
                    .any(|allowed| host_matches(allowed, host))
            })
            && candidate
                .paths
                .iter()
                .any(|pattern| path_matches(pattern, path))
            && candidate
                .tools
                .iter()
                .any(|candidate_tool| candidate_tool == "*" || candidate_tool == tool)
    });
    if allowed && !denied {
        Ok(())
    } else {
        Err("MCP tool is not allowed by the agent profile")
    }
}

fn filter_mcp_tools_list_response(
    body: &[u8],
    policy: &ProxyPolicy,
    host: Option<&str>,
    path: &str,
) -> Result<Bytes> {
    let mut value: serde_json::Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return filter_mcp_tools_list_sse_response(body, policy, host, path),
    };
    let Some(tools) = value
        .get_mut("result")
        .and_then(|result| result.get_mut("tools"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(Bytes::copy_from_slice(body));
    };
    tools.retain(|tool| {
        let Some(name) = tool.get("name").and_then(serde_json::Value::as_str) else {
            return false;
        };
        let allowed = policy.mcp_rules.iter().any(|rule| {
            rule.effect == AgentHttpRuleEffect::Allow
                && host.is_some_and(|host| {
                    rule.hosts.iter().any(|allowed| host_matches(allowed, host))
                })
                && rule.paths.iter().any(|pattern| path_matches(pattern, path))
                && rule
                    .tools
                    .iter()
                    .any(|candidate| candidate == "*" || candidate == name)
        });
        let denied = policy.mcp_rules.iter().any(|rule| {
            rule.effect == AgentHttpRuleEffect::Deny
                && host.is_some_and(|host| {
                    rule.hosts.iter().any(|allowed| host_matches(allowed, host))
                })
                && rule.paths.iter().any(|pattern| path_matches(pattern, path))
                && rule
                    .tools
                    .iter()
                    .any(|candidate| candidate == "*" || candidate == name)
        });
        allowed && !denied
    });
    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn filter_mcp_tools_list_sse_response(
    body: &[u8],
    policy: &ProxyPolicy,
    host: Option<&str>,
    path: &str,
) -> Result<Bytes> {
    let text = String::from_utf8_lossy(body);
    let mut output = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if let Some(data) = line.strip_prefix("data:") {
            let leading = &data[..data.len() - data.trim_start().len()];
            if let Ok(filtered) =
                filter_mcp_tools_list_response(data.trim_start().as_bytes(), policy, host, path)
            {
                output.push_str("data:");
                output.push_str(leading);
                output.push_str(&String::from_utf8_lossy(&filtered));
                if line.ends_with('\n') {
                    output.push('\n');
                }
                continue;
            }
        }
        output.push_str(line);
    }
    Ok(Bytes::from(output))
}

fn policy_denies_host(policy: &ProxyPolicy, host: &str) -> bool {
    policy
        .denied_hosts
        .iter()
        .any(|denied| denied == "*" || host_matches(denied, host))
}

fn replace_placeholder(
    request: &mut Request<Incoming>,
    state: &ProxyState,
    host: Option<&str>,
) -> std::result::Result<Option<String>, CredentialDenial> {
    for (placeholder, secret) in state.secrets.iter() {
        let injection = state
            .policy
            .secret_injections
            .get(placeholder)
            .cloned()
            .unwrap_or_else(SecretInjection::bearer);
        let Ok(header_name) = HeaderName::from_bytes(injection.header.as_bytes()) else {
            continue;
        };
        let expected = injection.value_template.replace("{value}", placeholder);
        let matches_placeholder = request
            .headers()
            .get(&header_name)
            .and_then(|value| value.to_str().ok())
            == Some(expected.as_str());
        if !matches_placeholder {
            continue;
        }

        if state.policy.strict_deny
            && !secret_allows_request(
                &state.policy,
                placeholder,
                host,
                request.method(),
                request.uri().path(),
            )
        {
            return Err(CredentialDenial {
                secret_name: secret_name_from_placeholder(placeholder),
                audit_action: credential_denial_action(&state.policy, placeholder),
            });
        }
        if state.remote.is_some() {
            return Ok(Some(secret_name_from_placeholder(placeholder)));
        }
        let value = injection.value_template.replace("{value}", secret);
        if let Ok(value) = HeaderValue::from_str(&value) {
            request.headers_mut().insert(header_name, value);
        }
        return Ok(Some(secret_name_from_placeholder(placeholder)));
    }

    Ok(None)
}

/// Internal-only detail for audit classification. The agent receives the same
/// generic credential-denied response for every variant.
struct CredentialDenial {
    secret_name: String,
    audit_action: &'static str,
}

fn credential_denial_action(policy: &ProxyPolicy, placeholder: &str) -> &'static str {
    match policy.secret_policies.get(placeholder) {
        Some(SecretHttpPolicy::Rules(_)) => "credential_rule_denied",
        Some(SecretHttpPolicy::LegacyHosts(_)) | None => "host_denied",
    }
}

fn secret_allows_request(
    policy: &ProxyPolicy,
    placeholder: &str,
    host: Option<&str>,
    method: &Method,
    path: &str,
) -> bool {
    let Some(host) = host else { return false };
    policy
        .secret_policies
        .get(placeholder)
        .is_some_and(|secret_policy| {
            matches!(
                evaluate_secret_authorization(secret_policy, host, method.as_str(), path),
                SecretAuthorizationDecision::AllowedLegacyHost
                    | SecretAuthorizationDecision::AllowedRule
            )
        })
}

/// Reject placeholder-shaped values that do not belong to this session instead
/// of forwarding them to an upstream service. This avoids accidental leakage of
/// a placeholder and makes stale profile bindings diagnosable from audit logs.
fn contains_unknown_placeholder(request: &Request<Incoming>, state: &ProxyState) -> bool {
    request.headers().values().any(|value| {
        let Ok(value) = value.to_str() else {
            return false;
        };
        let mut remaining = value;
        while let Some(index) = remaining.find("**STASHBASE_") {
            let candidate = &remaining[index..];
            let Some(end) = candidate[2..].find("**") else {
                return false;
            };
            let placeholder = &candidate[..end + 4];
            if !state.secrets.contains_key(placeholder) {
                return true;
            }
            remaining = &candidate[end + 4..];
        }
        if state.remote.is_some() {
            for candidate in value.match_indices("${") {
                let suffix = &value[candidate.0..];
                let Some(end) = suffix.find('}') else {
                    // An unclosed "${" is not a well-formed placeholder; skip it
                    // rather than treating it as an unknown credential. Headers
                    // can legitimately contain shell templates, JS interpolation
                    // strings, or other "${"-prefixed content without a closing
                    // brace, and rejecting them would produce a false-positive 403.
                    continue;
                };
                if !state.secrets.contains_key(&suffix[..=end]) {
                    return true;
                }
            }
        }
        false
    })
}

fn upstream_error_action(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "upstream_timeout"
    } else if error.is_connect() {
        "upstream_connection_failed"
    } else {
        "upstream_request_failed"
    }
}

fn request_host(request: &Request<Incoming>, connect_authority: Option<&str>) -> Option<String> {
    connect_authority
        .map(host_from_authority)
        .map(str::to_owned)
        .or_else(|| request.uri().host().map(str::to_owned))
        .or_else(|| {
            request
                .headers()
                .get("host")
                .and_then(|value| value.to_str().ok())
                .map(host_from_authority)
                .map(str::to_owned)
        })
}

fn host_from_authority(authority: &str) -> &str {
    if let Some(bracketed) = authority.strip_prefix('[') {
        if let Some((host, _)) = bracketed.split_once(']') {
            return host;
        }
    }
    authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
}

fn normalize_hosts(hosts: HashSet<String>) -> HashSet<String> {
    hosts
        .into_iter()
        .map(|host| host.trim().trim_end_matches('.').to_ascii_lowercase())
        .collect()
}

fn normalize_injections(
    injections: HashMap<String, SecretInjection>,
    remote_placeholders: Option<&HashMap<String, String>>,
) -> Result<HashMap<String, SecretInjection>> {
    injections
        .into_iter()
        .map(|(name, injection)| {
            let header = HeaderName::from_bytes(injection.header.as_bytes())
                .with_context(|| format!("invalid credential header for secret '{name}'"))?;
            if !injection.value_template.contains("{value}") {
                anyhow::bail!(
                    "credential value template for binding '{name}' must contain '{{value}}'"
                );
            }
            let placeholder = remote_placeholders
                .and_then(|placeholders| placeholders.get(&name))
                .cloned()
                .unwrap_or_else(|| placeholder_for(&name));
            Ok((
                placeholder,
                SecretInjection {
                    header: header.as_str().to_owned(),
                    value_template: injection.value_template,
                },
            ))
        })
        .collect()
}

/// Proxy failures use the public API error envelope so a nested `stashbase`
/// command can report policy denials clearly instead of failing JSON parsing.
fn proxy_error_response(status: StatusCode, code: &str, message: &str) -> Response<ProxyBody> {
    proxy_error_response_with_id(status, code, message, None)
}

fn new_local_request_id() -> String {
    new_local_audit_event_id()
}

fn new_local_audit_event_id() -> String {
    format!("evt_{}", ShortUuid::generate())
}

fn proxy_error_response_with_id(
    status: StatusCode,
    code: &str,
    message: &str,
    id: Option<&str>,
) -> Response<ProxyBody> {
    let body = serde_json::json!({
        "error": {
            "code": code,
            "message": message,
            "id": id,
        }
    })
    .to_string();
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(full_body(Bytes::from(body)))
        .unwrap()
}

fn full_body(body: Bytes) -> ProxyBody {
    Full::new(body)
        .map_err(|never| -> BoxError { match never {} })
        .boxed_unsync()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{stream, StreamExt};
    use hyper::header::{AUTHORIZATION, HOST, LOCATION, TRANSFER_ENCODING};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::{mpsc, oneshot},
        time::{sleep, timeout},
    };

    async fn start_backend() -> (std::net::SocketAddr, oneshot::Receiver<Option<String>>) {
        start_backend_capturing(AUTHORIZATION).await
    }

    async fn start_backend_capturing(
        header_name: HeaderName,
    ) -> (std::net::SocketAddr, oneshot::Receiver<Option<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (authorization, receiver) = oneshot::channel();
        let authorization = Arc::new(std::sync::Mutex::new(Some(authorization)));
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: Request<Incoming>| {
                let authorization = authorization.clone();
                let header_name = header_name.clone();
                let value = request
                    .headers()
                    .get(&header_name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                if let Some(sender) = authorization.lock().unwrap().take() {
                    let _ = sender.send(value);
                }
                async move {
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(StatusCode::NO_CONTENT)
                            .body(Full::new(Bytes::new()))
                            .unwrap(),
                    )
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        (address, receiver)
    }

    fn proxy_client(proxy: &Proxy) -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(&proxy.child_env()["HTTP_PROXY"]).unwrap())
            .build()
            .unwrap()
    }

    fn mcp_test_policy() -> ProxyPolicy {
        ProxyPolicy {
            secret_policies: HashMap::new(),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: true,
            mcp_rules: vec![
                AgentMcpRule {
                    effect: AgentHttpRuleEffect::Allow,
                    hosts: vec!["mcp.example.com".to_owned()],
                    paths: vec!["/mcp".to_owned()],
                    tools: vec!["*".to_owned()],
                },
                AgentMcpRule {
                    effect: AgentHttpRuleEffect::Deny,
                    hosts: vec!["mcp.example.com".to_owned()],
                    paths: vec!["/mcp".to_owned()],
                    tools: vec!["list_projects".to_owned()],
                },
            ],
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        }
    }

    #[test]
    fn mcp_tools_list_hides_denied_tools_for_fresh_sessions() {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"tools": [
                {"name": "list_projects"},
                {"name": "list_project_labels"}
            ]}
        });
        let filtered = filter_mcp_tools_list_response(
            serde_json::to_string(&body).unwrap().as_bytes(),
            &mcp_test_policy(),
            Some("mcp.example.com"),
            "/mcp",
        )
        .unwrap();

        let value: serde_json::Value = serde_json::from_slice(&filtered).unwrap();
        let names = value["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["list_project_labels"]);
    }

    #[test]
    fn mcp_sse_tools_list_hides_denied_tools_for_fresh_sessions() {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"tools": [
                {"name": "list_projects"},
                {"name": "list_project_labels"}
            ]}
        });
        let sse = format!("event: message\ndata: {}\n\n", body);
        let filtered = filter_mcp_tools_list_response(
            sse.as_bytes(),
            &mcp_test_policy(),
            Some("mcp.example.com"),
            "/mcp",
        )
        .unwrap();

        let text = String::from_utf8(filtered.to_vec()).unwrap();
        assert!(!text.contains("list_projects"));
        assert!(text.contains("list_project_labels"));
    }

    #[tokio::test]
    async fn mcp_sse_tools_list_is_filtered_incrementally() {
        let first = Bytes::from(format!(
            "data: {}\n\n",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"tools": [
                    {"name": "list_projects"},
                    {"name": "list_project_labels"}
                ]}
            })
        ));
        let source = stream::iter([Ok::<_, reqwest::Error>(first)]).chain(stream::once(async {
            sleep(Duration::from_millis(200)).await;
            Ok(Bytes::from_static(b": keepalive\n\n"))
        }));
        let mut filtered = McpToolsListSseStream::new(
            source,
            mcp_test_policy(),
            Some("mcp.example.com".to_owned()),
            "/mcp".to_owned(),
        );

        let first = timeout(Duration::from_millis(100), filtered.next())
            .await
            .expect("the first filtered SSE event was buffered")
            .unwrap()
            .unwrap();
        let text = String::from_utf8(first.to_vec()).unwrap();
        assert!(!text.contains("list_projects"));
        assert!(text.contains("list_project_labels"));
        assert_eq!(
            filtered.next().await.unwrap().unwrap(),
            Bytes::from_static(b": keepalive\n\n")
        );
    }

    #[test]
    fn mcp_inspection_requires_the_proxy_token() {
        let mut headers = HeaderMap::new();
        headers.insert(MCP_INSPECTION_HEADER, HeaderValue::from_static("1"));
        assert!(!is_authorized_mcp_inspection(&headers, "secret-token"));

        headers.insert(
            MCP_INSPECTION_HEADER,
            HeaderValue::from_static("secret-token"),
        );
        assert!(is_authorized_mcp_inspection(&headers, "secret-token"));
    }

    #[test]
    fn mcp_tools_call_allows_wildcard_tools_except_explicit_denials() {
        let allowed = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "list_project_labels", "arguments": {}}
        });
        let denied = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "list_projects", "arguments": {}}
        });
        let policy = mcp_test_policy();

        assert_eq!(
            authorize_mcp_request(
                serde_json::to_string(&allowed).unwrap().as_bytes(),
                &policy,
                Some("mcp.example.com"),
                "/mcp",
            ),
            Ok(())
        );
        assert_eq!(
            authorize_mcp_request(
                serde_json::to_string(&denied).unwrap().as_bytes(),
                &policy,
                Some("mcp.example.com"),
                "/mcp",
            ),
            Err("MCP tool is not allowed by the agent profile")
        );
    }

    #[test]
    fn mcp_tools_call_is_not_policy_checked_for_other_endpoints() {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "list_projects", "arguments": {}}
        });

        assert_eq!(
            authorize_mcp_request(
                serde_json::to_string(&request).unwrap().as_bytes(),
                &mcp_test_policy(),
                Some("mcp.example.com"),
                "/other",
            ),
            Ok(())
        );
    }

    #[test]
    fn mcp_json_rpc_responses_are_allowed() {
        let responses = [
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"model": "example"}
            }),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "error": {"code": -32601, "message": "Method not found"}
            }),
        ];

        for response in responses {
            assert_eq!(
                authorize_mcp_request(
                    serde_json::to_string(&response).unwrap().as_bytes(),
                    &mcp_test_policy(),
                    Some("mcp.example.com"),
                    "/mcp",
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn remote_session_token_is_redacted_and_expires_for_new_connections() {
        let config = RemoteProxyConfig {
            proxy_url: "https://proxy.example".to_owned(),
            session: Arc::new(RwLock::new(RemoteProxySessionState {
                token: "do-not-log".to_owned(),
                expires_at: Utc::now() - chrono::Duration::seconds(1),
                last_rotation_error: Some("temporary control-plane failure".to_owned()),
            })),
            placeholders: HashMap::new(),
            child_env: HashMap::new(),
            protocol: RemoteProxyProtocol::ForwardProxyTlsIntercept,
            ca_file: None,
            routing: Arc::new(RwLock::new(RemoteRouting::full())),
        };

        assert!(!format!("{config:?}").contains("do-not-log"));
        assert!(config
            .token_for_new_connection()
            .unwrap_err()
            .to_string()
            .contains("temporary control-plane failure"));
    }

    #[test]
    fn custom_remote_placeholder_uses_the_configured_child_environment_name() {
        let placeholder = "sk-ant-api03-stashbase-placeholder";
        let binding_names =
            HashMap::from([(placeholder.to_owned(), "ANTHROPIC_API_KEY".to_owned())]);
        let child_env = HashMap::from([(
            "ANTHROPIC_API_KEY".to_owned(),
            "ANTHROPIC_API_KEY".to_owned(),
        )]);

        assert_eq!(
            child_env_name_for_placeholder(placeholder, Some(&binding_names), Some(&child_env)),
            "ANTHROPIC_API_KEY"
        );
    }

    #[test]
    fn remote_proxy_host_header_uses_the_proxy_authority() {
        let proxy = reqwest::Url::parse("https://proxy.example:8443/v1/proxy").unwrap();

        assert_eq!(
            proxy_host_header(&proxy).unwrap(),
            HeaderValue::from_static("proxy.example:8443")
        );
    }

    #[test]
    fn remote_custom_header_injection_uses_its_remote_placeholder() {
        let injections = HashMap::from([(
            "ANTHROPIC_API_KEY".to_owned(),
            SecretInjection {
                header: "x-api-key".to_owned(),
                value_template: "{value}".to_owned(),
            },
        )]);
        let placeholders = HashMap::from([(
            "ANTHROPIC_API_KEY".to_owned(),
            "sk-ant-api03-stashbase-placeholder".to_owned(),
        )]);

        let normalized = normalize_injections(injections, Some(&placeholders)).unwrap();

        assert!(normalized.contains_key("sk-ant-api03-stashbase-placeholder"));
        assert!(!normalized.contains_key("**STASHBASE_ANTHROPIC_API_KEY**"));
    }

    #[test]
    fn remote_proxy_ca_cache_verifies_and_writes_the_session_certificate() {
        let directory =
            std::env::temp_dir().join(format!("stashbase-remote-ca-{}", Uuid::new_v4()));
        let (_, generated_path) = create_certificate_authority().unwrap();
        let pem = fs::read_to_string(&generated_path).unwrap();
        let certificate = crate::api::remote_proxy::RemoteProxyCa {
            key_id: "test-ca".to_owned(),
            sha256: format!("{:x}", Sha256::digest(pem.as_bytes())),
            pem: pem.clone(),
        };

        let path = provision_remote_proxy_ca_at(&directory, &certificate).unwrap();

        assert_eq!(path.file_name().unwrap(), "remote-proxy-test-ca.pem");
        assert_eq!(fs::read_to_string(&path).unwrap(), pem);
        assert!(!directory.join("proxy-ca.json").exists());

        let invalid = crate::api::remote_proxy::RemoteProxyCa {
            sha256: "0".repeat(64),
            ..certificate
        };
        assert!(provision_remote_proxy_ca_at(&directory, &invalid).is_err());
        assert!(remote_proxy_ca_path(&directory, "../unsafe").is_err());
        fs::remove_file(generated_path).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn authority_parser_handles_bracketed_ipv6() {
        assert_eq!(host_from_authority("[::1]:443"), "::1");
        assert_eq!(
            host_from_authority("api.example.com:443"),
            "api.example.com"
        );
    }

    #[test]
    fn remote_upgrade_uses_origin_form_after_connect() {
        let request = Request::builder()
            .uri("http://api.example.com/ws?stream=true")
            .body(())
            .unwrap();

        assert_eq!(
            upgrade_origin_form_uri(&request),
            "/ws?stream=true".parse::<hyper::Uri>().unwrap()
        );
    }

    #[tokio::test]
    async fn custom_remote_forward_uses_the_remote_proxy_host_header() {
        let (address, host) = start_backend_capturing(HOST).await;
        let remote = RemoteProxyConfig {
            proxy_url: format!("http://{address}/v1/agent-proxy/proxy"),
            session: Arc::new(RwLock::new(RemoteProxySessionState {
                token: "session-token".to_owned(),
                expires_at: Utc::now() + chrono::Duration::minutes(10),
                last_rotation_error: None,
            })),
            placeholders: HashMap::new(),
            child_env: HashMap::new(),
            protocol: RemoteProxyProtocol::Custom,
            ca_file: None,
            routing: Arc::new(RwLock::new(RemoteRouting::full())),
        };
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .get("http://original.example/path")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let expected_host = address.to_string();
        assert_eq!(host.await.unwrap().as_deref(), Some(expected_host.as_str()));
        proxy.stop().await;
    }

    #[tokio::test]
    async fn remote_custom_header_is_denied_before_reaching_the_remote_proxy() {
        let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote_address = remote_listener.local_addr().unwrap();
        let remote = RemoteProxyConfig {
            proxy_url: format!("http://{remote_address}/v1/agent-proxy/proxy"),
            session: Arc::new(RwLock::new(RemoteProxySessionState {
                token: "session-token".to_owned(),
                expires_at: Utc::now() + chrono::Duration::minutes(10),
                last_rotation_error: None,
            })),
            placeholders: HashMap::from([(
                "ANTHROPIC_API_KEY".to_owned(),
                "sk-ant-api03-stashbase-placeholder".to_owned(),
            )]),
            child_env: HashMap::new(),
            protocol: RemoteProxyProtocol::Custom,
            ca_file: None,
            routing: Arc::new(RwLock::new(RemoteRouting::full())),
        };
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "ANTHROPIC_API_KEY".to_owned(),
                SecretHttpPolicy::LegacyHosts(HashSet::from(["api.anthropic.com".to_owned()])),
            )]),
            secret_injections: HashMap::from([(
                "ANTHROPIC_API_KEY".to_owned(),
                SecretInjection {
                    header: "x-api-key".to_owned(),
                    value_template: "{value}".to_owned(),
                },
            )]),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: true,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };
        let proxy = Proxy::start_remote_with_port(remote, policy, None, None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .get("http://unapproved.example/test")
            .header("x-api-key", "sk-ant-api03-stashbase-placeholder")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response.bytes().await.unwrap();
        let error: crate::models::api_client::ApiErrorResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(error.error.code, "proxy.credential_not_allowed");
        assert!(
            timeout(Duration::from_millis(100), remote_listener.accept())
                .await
                .is_err()
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn remote_relay_decision_matches_agent_explain_evaluator() {
        let rules = vec![
            AgentHttpRule {
                effect: AgentHttpRuleEffect::Allow,
                hosts: vec!["api.github.com".to_owned()],
                methods: vec!["GET".to_owned()],
                paths: vec!["/user".to_owned()],
            },
            AgentHttpRule {
                effect: AgentHttpRuleEffect::Deny,
                hosts: vec!["api.github.com".to_owned()],
                methods: vec!["DELETE".to_owned()],
                paths: vec!["*".to_owned()],
            },
        ];
        let explain_policy = SecretHttpPolicy::Rules(rules.clone());
        assert_eq!(
            evaluate_secret_authorization(&explain_policy, "api.github.com", "GET", "/user"),
            SecretAuthorizationDecision::AllowedRule
        );
        assert_eq!(
            evaluate_secret_authorization(&explain_policy, "api.github.com", "DELETE", "/user"),
            SecretAuthorizationDecision::DeniedRule
        );

        let (allowed_address, _authorization) = start_backend().await;
        let allowed_proxy = Proxy::start_remote_with_port(
            remote_test_config(allowed_address),
            remote_rule_policy(rules.clone()),
            None,
            None,
        )
        .await
        .unwrap();
        let allowed_response = proxy_client(&allowed_proxy)
            .get("http://api.github.com/user")
            .header(AUTHORIZATION, "Bearer ${STASHBASE_GITHUB_TOKEN}")
            .send()
            .await
            .unwrap();
        assert_eq!(allowed_response.status(), StatusCode::NO_CONTENT);
        allowed_proxy.stop().await;

        let denied_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let denied_proxy = Proxy::start_remote_with_port(
            remote_test_config(denied_listener.local_addr().unwrap()),
            remote_rule_policy(rules),
            None,
            None,
        )
        .await
        .unwrap();
        let denied_response = proxy_client(&denied_proxy)
            .delete("http://api.github.com/user")
            .header(AUTHORIZATION, "Bearer ${STASHBASE_GITHUB_TOKEN}")
            .send()
            .await
            .unwrap();
        assert_eq!(denied_response.status(), StatusCode::FORBIDDEN);
        assert!(
            timeout(Duration::from_millis(100), denied_listener.accept())
                .await
                .is_err()
        );
        denied_proxy.stop().await;
    }

    fn remote_test_config(address: std::net::SocketAddr) -> RemoteProxyConfig {
        RemoteProxyConfig {
            proxy_url: format!("http://{address}/v1/agent-proxy/proxy"),
            session: Arc::new(RwLock::new(RemoteProxySessionState {
                token: "session-token".to_owned(),
                expires_at: Utc::now() + chrono::Duration::minutes(10),
                last_rotation_error: None,
            })),
            placeholders: HashMap::from([(
                "GITHUB_TOKEN".to_owned(),
                "${STASHBASE_GITHUB_TOKEN}".to_owned(),
            )]),
            child_env: HashMap::new(),
            protocol: RemoteProxyProtocol::Custom,
            ca_file: None,
            routing: Arc::new(RwLock::new(RemoteRouting::full())),
        }
    }

    fn remote_rule_policy(rules: Vec<AgentHttpRule>) -> ProxyPolicy {
        ProxyPolicy {
            secret_policies: HashMap::from([(
                "GITHUB_TOKEN".to_owned(),
                SecretHttpPolicy::Rules(rules),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: true,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        }
    }

    #[tokio::test]
    async fn https_remote_proxy_connection_starts_with_tls() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (hello_sender, hello) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 3];
            stream.read_exact(&mut bytes).await.unwrap();
            let _ = hello_sender.send(bytes);
        });

        let proxy =
            reqwest::Url::parse(&format!("https://127.0.0.1:{}/proxy", address.port())).unwrap();
        let connect = tokio::spawn(async move { connect_remote_proxy(&proxy).await });

        assert_eq!(
            timeout(Duration::from_secs(1), hello)
                .await
                .unwrap()
                .unwrap()[0],
            0x16
        );
        assert!(connect.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn remote_connect_rejection_is_reported_before_a_child_tunnel_opens() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let remote = RemoteProxyConfig {
            proxy_url: format!("http://{address}"),
            session: Arc::new(RwLock::new(RemoteProxySessionState {
                token: "session-token".to_owned(),
                expires_at: Utc::now() + chrono::Duration::minutes(10),
                last_rotation_error: None,
            })),
            placeholders: HashMap::new(),
            child_env: HashMap::new(),
            protocol: RemoteProxyProtocol::ForwardProxyTlsIntercept,
            ca_file: None,
            routing: Arc::new(RwLock::new(RemoteRouting::full())),
        };

        let error = match establish_remote_connect("api.example.com:443", &remote).await {
            Ok(_) => panic!("rejected remote CONNECT should not open a tunnel"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("rejected CONNECT"));
    }

    fn credential_routing(hosts: &[&str]) -> Arc<RwLock<RemoteRouting>> {
        let hosts = hosts
            .iter()
            .map(|host| (*host).to_owned())
            .collect::<Vec<_>>();
        Arc::new(RwLock::new(
            RemoteRouting::resolve(Some(RemoteMode::Credential), Some(&hosts), [], true).unwrap(),
        ))
    }

    #[tokio::test]
    async fn credential_routing_sends_a_listed_host_through_the_remote_proxy() {
        let (remote_address, target) =
            start_backend_capturing(HeaderName::from_static("x-stashbase-target")).await;
        let mut remote = remote_test_config(remote_address);
        remote.routing = credential_routing(&["original.example"]);
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .get("http://original.example/path")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            target.await.unwrap().as_deref(),
            Some("http://original.example/path")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn credential_routing_sends_an_unlisted_host_direct() {
        let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (destination, _) = start_backend().await;
        let mut remote = remote_test_config(remote_listener.local_addr().unwrap());
        remote.routing = credential_routing(&["original.example"]);
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{destination}/"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            timeout(Duration::from_millis(100), remote_listener.accept())
                .await
                .is_err(),
            "a direct request must not reach the remote proxy"
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn a_credential_placeholder_is_refused_on_a_direct_host() {
        let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut remote = remote_test_config(remote_listener.local_addr().unwrap());
        remote.routing = credential_routing(&["api.github.com"]);
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{}/", destination.local_addr().unwrap()))
            .header(AUTHORIZATION, "Bearer ${STASHBASE_GITHUB_TOKEN}")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("proxy.credential_host_not_routed"));
        assert!(
            timeout(Duration::from_millis(100), destination.accept())
                .await
                .is_err(),
            "the placeholder must never be forwarded to a direct destination"
        );
        assert!(
            timeout(Duration::from_millis(100), remote_listener.accept())
                .await
                .is_err()
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn credential_routing_opens_a_direct_connect_tunnel_without_the_remote_proxy() {
        // A forward-proxy session must not CONNECT through the remote proxy for
        // a host that credential routing sends direct.
        let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut remote = remote_test_config(remote_listener.local_addr().unwrap());
        remote.protocol = RemoteProxyProtocol::ForwardProxyTlsIntercept;
        let (_, ca_path) = create_certificate_authority().unwrap();
        remote.ca_file = Some(ca_path);
        remote.routing = credential_routing(&["api.github.com"]);
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();
        let address = proxy.child_env()["HTTP_PROXY"]
            .trim_start_matches("http://")
            .to_owned();

        let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
        stream
            .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .await
            .unwrap();
        let mut response = [0; 64];
        let read = timeout(Duration::from_secs(2), stream.read(&mut response))
            .await
            .unwrap()
            .unwrap();

        assert!(String::from_utf8_lossy(&response[..read]).starts_with("HTTP/1.1 200"));
        assert!(
            timeout(Duration::from_millis(100), remote_listener.accept())
                .await
                .is_err(),
            "a direct host must not open a CONNECT to the remote proxy"
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn credential_routing_trusts_both_the_local_and_remote_ca_in_forward_proxy_mode() {
        let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut remote = remote_test_config(remote_listener.local_addr().unwrap());
        remote.protocol = RemoteProxyProtocol::ForwardProxyTlsIntercept;
        let (_, remote_ca_path) = create_certificate_authority().unwrap();
        remote.ca_file = Some(remote_ca_path.clone());
        remote.routing = credential_routing(&["api.github.com"]);
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();

        let bundle = fs::read_to_string(&proxy.child_env()["SSL_CERT_FILE"]).unwrap();
        assert_eq!(bundle.matches("BEGIN CERTIFICATE").count(), 2);
        assert!(bundle.contains(fs::read_to_string(&remote_ca_path).unwrap().trim()));
        proxy.stop().await;
        let _ = fs::remove_file(remote_ca_path);
    }

    #[test]
    fn creates_expected_placeholders() {
        assert_eq!(placeholder_for("GH_TOKEN"), "**STASHBASE_GH_TOKEN**");
    }

    #[tokio::test]
    async fn unclosed_dollar_brace_in_header_is_not_treated_as_unknown_placeholder() {
        // An unclosed "${" (shell template, JS interpolation, etc.) must not
        // produce a false-positive 403 proxy.placeholder_not_allowed response.
        // Use a real backend listener as the remote proxy so a 502 connection
        // error cannot mask a 403 that slips through the placeholder check.
        let (proxy_address, _) = start_backend().await;
        let remote = RemoteProxyConfig {
            proxy_url: format!("http://{proxy_address}"),
            session: Arc::new(RwLock::new(RemoteProxySessionState {
                token: "token".to_owned(),
                expires_at: Utc::now() + chrono::Duration::minutes(10),
                last_rotation_error: None,
            })),
            placeholders: HashMap::from([(
                "ANTHROPIC_API_KEY".to_owned(),
                "${STASHBASE_ANTHROPIC_API_KEY}".to_owned(),
            )]),
            child_env: HashMap::new(),
            protocol: RemoteProxyProtocol::Custom,
            ca_file: None,
            routing: Arc::new(RwLock::new(RemoteRouting::full())),
        };
        let proxy = Proxy::start_remote_with_port(remote, ProxyPolicy::permissive(), None, None)
            .await
            .unwrap();

        // Header contains "${" with no closing "}" — must pass through, not 403.
        let response = proxy_client(&proxy)
            .get("http://original.example/")
            .header("x-template", "Hello ${name")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        proxy.stop().await;
    }

    #[tokio::test]
    async fn the_proxy_refuses_stashbase_telemetry_even_when_egress_is_unrestricted() {
        // A permissive policy allows every host, so only the telemetry rule can
        // refuse this. It is the boundary an agent cannot switch off by
        // clearing STASHBASE_SANDBOX in its own environment.
        let api_host =
            crate::telemetry::send::destination_host().expect("the destination has a host");
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .post(format!("http://{api_host}/v1/telemetry"))
            .body("{}")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response.bytes().await.unwrap();
        let error: crate::models::api_client::ApiErrorResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(error.error.code, "proxy.telemetry_not_allowed");
        proxy.stop().await;
    }

    #[tokio::test]
    async fn proxy_errors_use_the_api_error_envelope() {
        let response = proxy_error_response(
            StatusCode::FORBIDDEN,
            "proxy.host_not_allowed",
            "Agent Proxy policy denied destination",
        );
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let error: crate::models::api_client::ApiErrorResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(error.error.code, "proxy.host_not_allowed");
        assert_eq!(
            error.error.message.as_deref(),
            Some("Agent Proxy policy denied destination")
        );
    }

    #[tokio::test]
    async fn proxy_errors_can_include_a_local_id() {
        let response = proxy_error_response_with_id(
            StatusCode::FORBIDDEN,
            "proxy.credential_not_allowed",
            "Credential is not authorized for this request.",
            Some("evt_test"),
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["id"], "evt_test");
    }

    #[test]
    fn local_audit_event_ids_use_a_short_uuid() {
        let id = new_local_request_id();
        assert!(id.starts_with("evt_"));
        assert_eq!(id.len(), 26);
        assert!(!id.contains('-'));

        let id = new_local_audit_event_id();
        assert!(id.starts_with("evt_"));
        assert_eq!(id.len(), 26);
        assert!(!id.contains('-'));
    }

    #[tokio::test]
    async fn proxy_stop_closes_the_listener() {
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();
        let address: std::net::SocketAddr = proxy.child_env()["HTTP_PROXY"]
            .trim_start_matches("http://")
            .parse()
            .unwrap();

        assert!(tokio::net::TcpStream::connect(address).await.is_ok());
        proxy.stop().await;
        let mut closed = false;
        for _ in 0..10 {
            if tokio::net::TcpStream::connect(address).await.is_err() {
                closed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(closed, "proxy listener remained reachable after stop");
    }

    #[tokio::test]
    async fn revoked_proxy_rejects_requests_without_closing_the_listener() {
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();
        let address = proxy.child_env()["HTTP_PROXY"].trim_start_matches("http://");
        let marker =
            std::env::temp_dir().join(format!("stashbase-revoked-{}.json", Uuid::new_v4()));
        std::fs::write(
            &marker,
            r#"{"session_id":"ags_test","command":"agent","started_at":"","process_id":0,"process_started_at":"","revoked":false}"#,
        )
        .unwrap();
        proxy.set_revocation_path(marker.clone());

        let mut existing = tokio::net::TcpStream::connect(address).await.unwrap();
        sleep(Duration::from_millis(20)).await;
        std::fs::write(
            &marker,
            r#"{"session_id":"ags_test","command":"agent","started_at":"","process_id":0,"process_started_at":"","revoked":true}"#,
        )
        .unwrap();
        // The proxy drops the existing connection. Depending on timing the
        // peer sees that as a clean EOF or, on Windows, as a reset (WSAECONNRESET
        // 10054) — both mean the connection was closed.
        let mut closed = [0; 1];
        match timeout(Duration::from_secs(2), existing.read(&mut closed))
            .await
            .expect("existing connection was not closed after revocation")
        {
            Ok(count) => assert_eq!(count, 0, "existing connection received data"),
            Err(error) => assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ),
                "unexpected error reading the closed connection: {error}"
            ),
        }

        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET http://example.test/ HTTP/1.1\r\nHost: example.test\r\n\r\n")
            .await
            .unwrap();
        let mut response = [0; 128];
        let count = stream.read(&mut response).await.unwrap();
        assert!(String::from_utf8_lossy(&response[..count]).starts_with("HTTP/1.1 502"));
        proxy.stop().await;
        std::fs::remove_file(marker).unwrap();
    }

    #[tokio::test]
    async fn proxy_uses_an_explicit_local_port() {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);

        let proxy =
            Proxy::start_with_port(HashMap::new(), ProxyPolicy::permissive(), None, Some(port))
                .await
                .unwrap();

        assert_eq!(
            proxy.child_env()["HTTP_PROXY"],
            format!("http://127.0.0.1:{port}")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn proxy_binds_to_provided_host_instead_of_loopback() {
        // 0.0.0.0 is used here only to prove the parameter is honored
        // without depending on a real Docker network gateway in CI.
        let proxy = Proxy::start_with_hook_and_bind_host(
            HashMap::new(),
            ProxyPolicy::permissive(),
            None,
            None,
            None,
            "0.0.0.0",
        )
        .await
        .unwrap();

        assert!(proxy.child_env()["HTTP_PROXY"].starts_with("http://0.0.0.0:"));
        proxy.stop().await;
    }

    #[tokio::test]
    async fn proxy_rejects_port_zero_override() {
        let result =
            Proxy::start_with_port(HashMap::new(), ProxyPolicy::permissive(), None, Some(0)).await;
        let error = match result {
            Ok(_) => panic!("port zero should be rejected"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("between 1 and 65535"));
    }

    #[test]
    fn audit_log_records_metadata_without_credential_values() {
        let path =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}.jsonl", Uuid::new_v4()));
        let audit_log = ProxyAuditLog {
            session_id: "session".to_owned(),
            profile: "coding".to_owned(),
            policy_fingerprint: "policy-fingerprint".to_owned(),
            profile_provenance: None,
            routing: None,
            binding_sources: Arc::new(HashMap::from([(
                "EXAMPLE_API_KEY".to_owned(),
                "personal_credential".to_owned(),
            )])),
            path: Arc::new(path.clone()),
            file: Arc::new(Mutex::new(
                OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .unwrap(),
            )),
        };

        audit_log.record(
            "injected",
            Some("api.example.com"),
            Some(&Method::POST),
            Some("EXAMPLE_API_KEY"),
            Some(StatusCode::OK),
            Some(Duration::from_millis(12)),
            Some("evt_test"),
        );
        let filesystem_event_id = audit_log.record_filesystem_denied(".env", "read");

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("api.example.com"));
        assert!(content.contains("EXAMPLE_API_KEY"));
        assert!(content.contains("personal_credential"));
        assert!(content.contains("policy-fingerprint"));
        let filesystem_event = content
            .lines()
            .map(|line| serde_json::from_str::<ProxyAuditLogEvent>(line).unwrap())
            .find(|event| event.event_id == filesystem_event_id)
            .unwrap();
        assert_eq!(filesystem_event.action, "filesystem_denied");
        assert_eq!(filesystem_event.path.as_deref(), Some(".env"));
        assert_eq!(filesystem_event.operation.as_deref(), Some("read"));
        assert!(!content.contains("real-secret-value"));
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn audit_response_stream_records_actual_relayed_byte_counts() {
        let path =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}.jsonl", Uuid::new_v4()));
        let audit_log = ProxyAuditLog {
            session_id: "session".to_owned(),
            profile: "coding".to_owned(),
            policy_fingerprint: "policy-fingerprint".to_owned(),
            profile_provenance: None,
            routing: None,
            binding_sources: Arc::new(HashMap::new()),
            path: Arc::new(path.clone()),
            file: Arc::new(Mutex::new(
                OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .unwrap(),
            )),
        };
        let request_bytes = Arc::new(AtomicU64::new(7));
        let mut response = AuditedResponseStream::new(
            stream::iter(vec![Ok::<_, reqwest::Error>(Bytes::from_static(b"hello"))]),
            request_bytes,
            Some(audit_log),
            "evt_test".to_owned(),
            "injected",
            Some("api.example.com".to_owned()),
            Some("/mcp".to_owned()),
            Method::POST,
            Some("EXAMPLE_API_KEY".to_owned()),
            Some("list_issues".to_owned()),
            StatusCode::OK,
            Instant::now(),
        );

        assert_eq!(
            response
                .next()
                .await
                .unwrap()
                .unwrap()
                .data_ref()
                .unwrap()
                .len(),
            5
        );
        assert!(response.next().await.is_none());
        drop(response);

        let event: ProxyAuditLogEvent =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(event.request_bytes, Some(7));
        assert_eq!(event.response_bytes, Some(5));
        assert_eq!(event.path.as_deref(), Some("/mcp"));
        assert_eq!(event.mcp_tool.as_deref(), Some("list_issues"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn audit_log_records_the_routing_mode_and_each_events_route() {
        let path =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}.jsonl", Uuid::new_v4()));
        let audit_log = ProxyAuditLog {
            session_id: "session".to_owned(),
            profile: "coding".to_owned(),
            policy_fingerprint: "policy-fingerprint".to_owned(),
            profile_provenance: None,
            binding_sources: Arc::new(HashMap::new()),
            routing: Some(credential_routing(&["api.github.com"])),
            path: Arc::new(path.clone()),
            file: Arc::new(Mutex::new(
                OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .unwrap(),
            )),
        };

        audit_log.record("session_started", None, None, None, None, None, None);
        for host in ["api.github.com", "example.com"] {
            audit_log.record(
                "forwarded",
                Some(host),
                Some(&Method::GET),
                None,
                None,
                None,
                None,
            );
        }

        let events = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<ProxyAuditLogEvent>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events[0].routing_mode.as_deref(), Some("credential"));
        assert_eq!(events[0].route, None);
        assert_eq!(events[1].routing_mode, None);
        assert_eq!(events[1].route.as_deref(), Some("remote"));
        assert_eq!(events[2].route.as_deref(), Some("direct"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn audit_log_records_profile_provenance_only_at_session_start() {
        let path =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}.jsonl", Uuid::new_v4()));
        let audit_log = ProxyAuditLog {
            session_id: "session".to_owned(),
            profile: "coding".to_owned(),
            policy_fingerprint: "policy-fingerprint".to_owned(),
            profile_provenance: Some(ProfileAuditProvenance {
                source: "./.stashbase/agents/coding.toml".to_owned(),
                modified_at: "2026-08-12T00:00:00+00:00".to_owned(),
                sha256: "profile-file-sha256".to_owned(),
            }),
            binding_sources: Arc::new(HashMap::new()),
            routing: None,
            path: Arc::new(path.clone()),
            file: Arc::new(Mutex::new(
                OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .unwrap(),
            )),
        };

        audit_log.record("session_started", None, None, None, None, None, None);
        audit_log.record(
            "forwarded",
            Some("api.github.com"),
            Some(&Method::GET),
            None,
            None,
            None,
            Some("evt_test"),
        );

        let events = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<ProxyAuditLogEvent>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(events[0].event_id.starts_with("evt_"));
        assert_eq!(events[1].event_id, "evt_test");
        assert_eq!(
            events[0].profile_source.as_deref(),
            Some("./.stashbase/agents/coding.toml")
        );
        assert_eq!(
            events[0].profile_file_sha256.as_deref(),
            Some("profile-file-sha256")
        );
        assert!(events[1].profile_source.is_none());
        assert!(events[1].profile_file_modified_at.is_none());
        assert!(events[1].profile_file_sha256.is_none());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn audit_log_pruning_reserves_a_slot_for_the_new_session() {
        let directory =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("unrelated.txt"), "keep me").unwrap();
        for index in 0..AUDIT_LOG_MAX_FILES {
            fs::write(directory.join(format!("agent-{index}.jsonl")), "{}").unwrap();
        }

        prune_proxy_audit_logs(&directory).unwrap();

        let retained = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("agent-"))
            .count();
        assert_eq!(retained, AUDIT_LOG_MAX_FILES - 1);
        assert!(directory.join("unrelated.txt").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn audit_log_filters_match_only_the_requested_metadata() {
        let event = ProxyAuditLogEvent {
            timestamp: "2026-01-01T00:00:00Z".to_owned(),
            session_id: "session-1".to_owned(),
            profile: "coding".to_owned(),
            policy_fingerprint: "policy-fingerprint".to_owned(),
            event_id: "evt_test".to_owned(),
            profile_source: None,
            profile_file_modified_at: None,
            profile_file_sha256: None,
            routing_mode: None,
            route: None,
            action: "injected".to_owned(),
            destination_host: Some("api.github.com".to_owned()),
            path: None,
            operation: None,
            mcp_tool: None,
            method: Some("POST".to_owned()),
            binding_name: Some("GH_TOKEN".to_owned()),
            binding_source: None,
            response_status: Some(200),
            duration_ms: Some(42),
            request_bytes: Some(10),
            response_bytes: Some(20),
        };

        assert!(ProxyAuditLogFilter {
            profile: Some("coding".to_owned()),
            action: Some("injected".to_owned()),
            host: Some("api.github.com".to_owned()),
            session: Some("session-1".to_owned()),
            id: Some("evt_test".to_owned()),
        }
        .matches(&event));
        assert!(!ProxyAuditLogFilter {
            host: Some("example.com".to_owned()),
            ..Default::default()
        }
        .matches(&event));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event_id"], "evt_test");
        assert!(json.get("id").is_none());
    }

    #[test]
    fn strict_policy_only_allows_secret_hosts_during_connect() {
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                SecretHttpPolicy::LegacyHosts(normalize_hosts(HashSet::from([
                    "API.GITHUB.COM.".to_owned()
                ]))),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };

        assert!(policy_allows_connect(&policy, "api.github.com"));
        assert!(!policy_allows_egress(&policy, "api.github.com"));
        assert!(!policy_allows_connect(&policy, "example.com"));
    }

    #[test]
    fn policy_supports_subdomain_wildcards_without_matching_the_apex() {
        assert!(host_matches("*.githubcopilot.com", "api.githubcopilot.com"));
        assert!(!host_matches("*.githubcopilot.com", "githubcopilot.com"));
        assert!(!host_matches(
            "*.githubcopilot.com",
            "evilgithubcopilot.com"
        ));
    }

    fn rule(effect: AgentHttpRuleEffect, methods: &[&str], paths: &[&str]) -> AgentHttpRule {
        AgentHttpRule {
            effect,
            hosts: vec!["api.github.com".to_owned()],
            methods: methods.iter().map(|method| (*method).to_owned()).collect(),
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    fn rule_policy(rules: Vec<AgentHttpRule>) -> ProxyPolicy {
        ProxyPolicy {
            secret_policies: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                SecretHttpPolicy::Rules(rules),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        }
    }

    #[test]
    fn policy_fingerprint_is_stable_for_equivalent_normalized_rules() {
        let left = rule_policy(vec![
            rule(
                AgentHttpRuleEffect::Allow,
                &["get", "POST"],
                &["/repos/*", "/user"],
            ),
            rule(AgentHttpRuleEffect::Deny, &["DELETE"], &["*"]),
        ]);
        let right = rule_policy(vec![
            rule(AgentHttpRuleEffect::Deny, &["delete"], &["*"]),
            rule(
                AgentHttpRuleEffect::Allow,
                &["POST", "GET"],
                &["/user", "/repos/*"],
            ),
        ]);

        assert_eq!(left.fingerprint(), right.fingerprint());
        assert_eq!(left.fingerprint().len(), 64);
    }

    #[test]
    fn policy_fingerprint_includes_normalized_mcp_rules() {
        let mut left = rule_policy(Vec::new());
        left.mcp_rules = vec![AgentMcpRule {
            effect: AgentHttpRuleEffect::Allow,
            hosts: vec!["MCP.EXAMPLE.COM".to_owned(), "other.example.com".to_owned()],
            paths: vec!["/second".to_owned(), "/mcp".to_owned()],
            tools: vec!["write".to_owned(), "read".to_owned()],
        }];
        let mut equivalent = rule_policy(Vec::new());
        equivalent.mcp_rules = vec![AgentMcpRule {
            effect: AgentHttpRuleEffect::Allow,
            hosts: vec!["other.example.com".to_owned(), "mcp.example.com".to_owned()],
            paths: vec!["/mcp".to_owned(), "/second".to_owned()],
            tools: vec!["read".to_owned(), "write".to_owned()],
        }];
        let mut different = equivalent.clone();
        different.mcp_rules[0].tools = vec!["read".to_owned()];

        assert_eq!(left.fingerprint(), equivalent.fingerprint());
        assert_ne!(left.fingerprint(), different.fingerprint());
    }

    #[test]
    fn policy_fingerprint_changes_with_sandbox_backend_and_settings() {
        // The sandbox backend and its Docker-specific settings are part of
        // the effective enforcement, not just the egress/secret policy —
        // an audit record must be able to distinguish a native run from a
        // Docker one, or one custom image from another, by fingerprint
        // alone.
        let native = rule_policy(Vec::new());
        let mut docker = native.clone();
        docker.backend = SandboxBackend::Docker;
        assert_ne!(native.fingerprint(), docker.fingerprint());

        let mut docker_image_a = docker.clone();
        docker_image_a.sandbox_image = Some("myorg/a:latest".to_owned());
        let mut docker_image_b = docker.clone();
        docker_image_b.sandbox_image = Some("myorg/b:latest".to_owned());
        assert_ne!(docker.fingerprint(), docker_image_a.fingerprint());
        assert_ne!(docker_image_a.fingerprint(), docker_image_b.fingerprint());

        let mut docker_dockerfile = docker.clone();
        docker_dockerfile.sandbox_dockerfile = Some("./custom.Dockerfile".to_owned());
        assert_ne!(docker.fingerprint(), docker_dockerfile.fingerprint());

        let mut docker_memory = docker.clone();
        docker_memory.sandbox_memory = Some("2g".to_owned());
        assert_ne!(docker.fingerprint(), docker_memory.fingerprint());
        let mut docker_worktree = docker.clone();
        docker_worktree.worktree = true;
        assert_ne!(docker.fingerprint(), docker_worktree.fingerprint());
        // Which worktree a run resumes is where it works, not what it may
        // do: the same policy either way.
        let mut resumed = docker_worktree.clone();
        resumed.worktree_resume = Some("amber-river-storm".to_owned());
        assert_eq!(docker_worktree.fingerprint(), resumed.fingerprint());
        assert_eq!(docker.worktree_request(), None);
        assert_eq!(
            docker_worktree.worktree_request(),
            Some(super::super::worktree::WorktreeRequest::New)
        );
        assert_eq!(
            resumed.worktree_request(),
            Some(super::super::worktree::WorktreeRequest::Resume(
                "amber-river-storm".to_owned()
            ))
        );

        let mut docker_cpus = docker.clone();
        docker_cpus.sandbox_cpus = Some("1.5".to_owned());
        assert_ne!(docker.fingerprint(), docker_cpus.fingerprint());

        let mut docker_isolated = docker.clone();
        docker_isolated.sandbox_isolated_paths = vec!["node_modules".to_owned()];
        assert_ne!(docker.fingerprint(), docker_isolated.fingerprint());
    }

    #[test]
    fn http_rules_allow_a_matching_request() {
        let policy = rule_policy(vec![rule(
            AgentHttpRuleEffect::Allow,
            &["get"],
            &["/repos/*"],
        )]);
        assert!(secret_allows_request(
            &policy,
            "**STASHBASE_GH_TOKEN**",
            Some("api.github.com"),
            &Method::GET,
            "/repos/acme/cli"
        ));
    }

    #[test]
    fn http_rules_default_deny_unmatched_routes_and_methods() {
        let policy = rule_policy(vec![rule(
            AgentHttpRuleEffect::Allow,
            &["GET"],
            &["/repos/*"],
        )]);
        assert!(!secret_allows_request(
            &policy,
            "**STASHBASE_GH_TOKEN**",
            Some("api.github.com"),
            &Method::GET,
            "/user"
        ));
        assert!(!secret_allows_request(
            &policy,
            "**STASHBASE_GH_TOKEN**",
            Some("api.github.com"),
            &Method::POST,
            "/repos/acme/cli"
        ));
    }

    #[test]
    fn http_rule_deny_overrides_allow_and_normalizes_dot_segments() {
        let policy = rule_policy(vec![
            rule(AgentHttpRuleEffect::Allow, &["GET"], &["/repos/*"]),
            rule(AgentHttpRuleEffect::Deny, &["GET"], &["/repos/private/*"]),
        ]);
        assert!(!secret_allows_request(
            &policy,
            "**STASHBASE_GH_TOKEN**",
            Some("api.github.com"),
            &Method::GET,
            "/repos/public/../private/repo"
        ));
    }

    #[test]
    fn audits_rule_denials_without_exposing_rule_details() {
        let policy = rule_policy(vec![rule(
            AgentHttpRuleEffect::Allow,
            &["GET"],
            &["/repos/*"],
        )]);
        assert_eq!(
            credential_denial_action(&policy, "**STASHBASE_GH_TOKEN**"),
            "credential_rule_denied"
        );
    }

    #[test]
    fn legacy_hosts_are_used_when_no_http_rules_exist() {
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                SecretHttpPolicy::LegacyHosts(HashSet::from(["api.github.com".to_owned()])),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };
        assert!(secret_allows_request(
            &policy,
            "**STASHBASE_GH_TOKEN**",
            Some("api.github.com"),
            &Method::DELETE,
            "/anything"
        ));
    }

    #[tokio::test]
    async fn dependency_hook_broker_is_opt_in_and_route_scoped() {
        let enabled = Proxy::start_with_hook(
            HashMap::new(),
            ProxyPolicy::permissive(),
            None,
            None,
            Some(HookBrokerConfig {
                api_key: "parent-api-key".to_owned(),
                dependency_check: true,
                secret_scan: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            enabled.child_env()[crate::api::dependencies::HOOK_MODE_ENV],
            "broker"
        );
        assert!(
            enabled.child_env()[crate::api::dependencies::HOOK_BROKER_URL_ENV]
                .ends_with(DEPENDENCY_HOOK_PATH)
        );
        assert!(!enabled.child_env().contains_key("STASHBASE_API_KEY"));
        assert!(!enabled.child_env()[crate::api::dependencies::HOOK_BROKER_TOKEN_ENV].is_empty());
        enabled.stop().await;

        let disabled =
            Proxy::start_with_hook(HashMap::new(), ProxyPolicy::permissive(), None, None, None)
                .await
                .unwrap();
        assert_eq!(
            disabled.child_env()[crate::api::dependencies::HOOK_MODE_ENV],
            "disabled"
        );
        assert!(!disabled
            .child_env()
            .contains_key(crate::api::dependencies::HOOK_BROKER_URL_ENV));
        disabled.stop().await;
    }

    #[cfg(unix)]
    fn scan_test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stashbase-scan-hook-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[cfg(unix)]
    fn fake_scan_exe(dir: &std::path::Path, exit_code: i32, sleep_secs: u32) -> PathBuf {
        let path = dir.join("fake-stashbase");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nsleep {sleep_secs}\n\
                 echo \"args=$*\"\necho \"cwd=$(pwd -P)\"\n\
                 echo \"key=$STASHBASE_API_KEY\"\necho \"api_url=$STASHBASE_API_URL\"\necho \"restricted=$STASHBASE_SCAN_RESTRICTED\"\n\
                 echo \"hook_token=${{STASHBASE_HOOK_BROKER_TOKEN:-unset}}\"\n\
                 echo 'finding on stderr' >&2\nexit {exit_code}\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    async fn start_scan_proxy(exe: PathBuf, workdir: PathBuf, timeout: Duration) -> Proxy {
        Proxy::start_with_hook(
            HashMap::new(),
            ProxyPolicy::permissive(),
            None,
            None,
            Some(HookBrokerConfig {
                api_key: "parent-api-key".to_owned(),
                dependency_check: false,
                secret_scan: Some(SecretScanConfig {
                    workdir,
                    timeout,
                    isolation: ScanIsolation::Unconfined { exe },
                }),
            }),
        )
        .await
        .unwrap()
    }

    fn hook_token(proxy: &Proxy) -> String {
        proxy.child_env()[crate::api::dependencies::HOOK_BROKER_TOKEN_ENV].clone()
    }

    async fn post_to(url: String, token: Option<&str>) -> (u16, String) {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut request = client.post(url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.unwrap();
        (response.status().as_u16(), response.text().await.unwrap())
    }

    #[cfg(unix)]
    async fn post_scan(proxy: &Proxy, mode: &str, token: Option<&str>) -> (u16, String) {
        let url = format!("{}/{mode}", proxy.child_env()[SCAN_BROKER_URL_ENV]);
        post_to(url, token).await
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secret_scan_hook_runs_host_scan_in_workdir_with_parent_key_restricted() {
        let dir = scan_test_dir();
        let proxy = start_scan_proxy(
            fake_scan_exe(&dir, 0, 0),
            dir.clone(),
            Duration::from_secs(10),
        )
        .await;

        let (status, body) = post_scan(&proxy, "staged", Some(&hook_token(&proxy))).await;

        assert_eq!(status, 200, "{body}");
        assert!(body.contains("args=scan staged --json --silent"), "{body}");
        assert!(body.contains(&format!("cwd={}", dir.display())), "{body}");
        assert!(body.contains("key=parent-api-key"), "{body}");
        assert!(
            body.contains(&format!("api_url={}", crate::api::client::get_api_url())),
            "{body}"
        );
        assert!(body.contains("restricted=1"), "{body}");
        assert!(body.contains("hook_token=unset"), "{body}");
        assert!(body.contains("finding on stderr"), "{body}");
        assert!(!proxy.child_env().contains_key("STASHBASE_API_KEY"));
        assert!(!proxy
            .child_env()
            .contains_key(crate::api::dependencies::HOOK_BROKER_URL_ENV));
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secret_scan_hook_maps_findings_to_422_and_supports_unpushed() {
        let dir = scan_test_dir();
        let proxy = start_scan_proxy(
            fake_scan_exe(&dir, 1, 0),
            dir.clone(),
            Duration::from_secs(10),
        )
        .await;

        let (status, body) = post_scan(&proxy, "unpushed", Some(&hook_token(&proxy))).await;

        assert_eq!(status, 422, "{body}");
        assert!(
            body.contains("args=scan unpushed --json --silent"),
            "{body}"
        );
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secret_scan_hook_rejects_bad_token_and_unknown_mode() {
        let dir = scan_test_dir();
        let proxy = start_scan_proxy(
            fake_scan_exe(&dir, 0, 0),
            dir.clone(),
            Duration::from_secs(10),
        )
        .await;
        let token = hook_token(&proxy);

        assert_eq!(post_scan(&proxy, "staged", None).await.0, 403);
        assert_eq!(post_scan(&proxy, "staged", Some("wrong")).await.0, 403);
        assert_eq!(post_scan(&proxy, "changes", Some(&token)).await.0, 404);
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn secret_scan_route_is_not_served_when_only_dependency_check_is_allowed() {
        let proxy = Proxy::start_with_hook(
            HashMap::new(),
            ProxyPolicy::permissive(),
            None,
            None,
            Some(HookBrokerConfig {
                api_key: "parent-api-key".to_owned(),
                dependency_check: true,
                secret_scan: None,
            }),
        )
        .await
        .unwrap();
        assert!(!proxy.child_env().contains_key(SCAN_BROKER_URL_ENV));
        let url = proxy.child_env()[crate::api::dependencies::HOOK_BROKER_URL_ENV].replace(
            DEPENDENCY_HOOK_PATH,
            &format!("{SECRET_SCAN_HOOK_PATH}/staged"),
        );

        let (status, _) = post_to(url, Some(&hook_token(&proxy))).await;

        assert_eq!(status, 403);
        proxy.stop().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dependency_route_is_not_served_when_only_secret_scan_is_allowed() {
        let dir = scan_test_dir();
        let proxy = start_scan_proxy(
            fake_scan_exe(&dir, 0, 0),
            dir.clone(),
            Duration::from_secs(10),
        )
        .await;
        let url = proxy.child_env()[SCAN_BROKER_URL_ENV]
            .replace(SECRET_SCAN_HOOK_PATH, DEPENDENCY_HOOK_PATH);

        let (status, _) = post_to(url, Some(&hook_token(&proxy))).await;

        assert_eq!(status, 403);
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secret_scan_hook_times_out_with_504() {
        let dir = scan_test_dir();
        let proxy = start_scan_proxy(
            fake_scan_exe(&dir, 0, 5),
            dir.clone(),
            Duration::from_millis(300),
        )
        .await;

        let (status, body) = post_scan(&proxy, "staged", Some(&hook_token(&proxy))).await;

        assert_eq!(status, 504, "{body}");
        assert!(body.contains("proxy.secret_scan_timeout"), "{body}");
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secret_scan_hook_caps_huge_output_and_still_finishes() {
        let dir = scan_test_dir();
        let exe = dir.join("loud-stashbase");
        std::fs::write(
            &exe,
            "#!/bin/sh\nhead -c 5242880 /dev/zero | tr '\\0' a\nhead -c 5242880 /dev/zero | tr '\\0' b >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let proxy = start_scan_proxy(exe, dir.clone(), Duration::from_secs(10)).await;

        let (status, body) = post_scan(&proxy, "staged", Some(&hook_token(&proxy))).await;

        // 422 rather than 504: the excess was drained, so the scan exited.
        assert_eq!(status, 422);
        assert!(body.len() <= SECRET_SCAN_BODY_LIMIT, "{}", body.len());
        assert!(body.starts_with('a'));
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn secret_scan_hooks_are_serialized() {
        let dir = scan_test_dir();
        let proxy = start_scan_proxy(
            fake_scan_exe(&dir, 0, 1),
            dir.clone(),
            Duration::from_secs(10),
        )
        .await;
        let token = hook_token(&proxy);

        let started = std::time::Instant::now();
        let (a, b) = tokio::join!(
            post_scan(&proxy, "staged", Some(&token)),
            post_scan(&proxy, "staged", Some(&token)),
        );

        assert_eq!((a.0, b.0), (200, 200));
        assert!(
            started.elapsed() >= Duration::from_secs(2),
            "scans ran concurrently"
        );
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn child_environment_uses_the_binding_name_for_its_default_placeholder() {
        let proxy = Proxy::start(
            HashMap::from([("GITHUB_TOKEN".to_owned(), "real-token".to_owned())]),
            ProxyPolicy::permissive(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            proxy.child_env().get("GITHUB_TOKEN").map(String::as_str),
            Some("**STASHBASE_GITHUB_TOKEN**")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn child_environment_marks_the_session_so_telemetry_stays_off() {
        // A Stashbase CLI run by the agent must never send telemetry, even if
        // the profile's egress policy would allow the API host.
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();

        assert_eq!(
            proxy
                .child_env()
                .get("STASHBASE_SANDBOX")
                .map(String::as_str),
            Some("1")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn redirects_reauthorize_the_credential_for_the_redirected_path() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests, mut received) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: Request<Incoming>| {
                let requests = requests.clone();
                async move {
                    let _ = requests.send(request.uri().path().to_owned());
                    let response = if request.uri().path() == "/allowed" {
                        Response::builder()
                            .status(StatusCode::FOUND)
                            .header(LOCATION, "/blocked")
                            .body(Full::new(Bytes::new()))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::NO_CONTENT)
                            .body(Full::new(Bytes::new()))
                            .unwrap()
                    };
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "GITHUB_TOKEN".to_owned(),
                SecretHttpPolicy::Rules(vec![
                    AgentHttpRule {
                        effect: AgentHttpRuleEffect::Allow,
                        hosts: vec!["127.0.0.1".to_owned()],
                        methods: vec!["GET".to_owned()],
                        paths: vec!["/allowed".to_owned()],
                    },
                    AgentHttpRule {
                        effect: AgentHttpRuleEffect::Deny,
                        hosts: vec!["127.0.0.1".to_owned()],
                        methods: vec!["GET".to_owned()],
                        paths: vec!["/blocked".to_owned()],
                    },
                ]),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };
        let proxy = Proxy::start(
            HashMap::from([("GITHUB_TOKEN".to_owned(), "real-token".to_owned())]),
            policy,
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/allowed"))
            .header(AUTHORIZATION, "Bearer **STASHBASE_GITHUB_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(received.recv().await.as_deref(), Some("/allowed"));
        assert!(timeout(Duration::from_millis(100), received.recv())
            .await
            .is_err());
        proxy.stop().await;
    }

    #[test]
    fn egress_wildcard_allows_any_destination_without_widening_secret_hosts() {
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                SecretHttpPolicy::LegacyHosts(HashSet::from(["api.github.com".to_owned()])),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: true,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };

        assert!(policy_allows_egress(&policy, "example.com"));
        assert!(matches!(
            &policy.secret_policies["**STASHBASE_GH_TOKEN**"],
            SecretHttpPolicy::LegacyHosts(hosts)
                if !hosts.iter().any(|allowed| host_matches(allowed, "example.com"))
        ));
    }

    #[test]
    fn denied_hosts_override_wildcard_egress_and_secret_destinations() {
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                SecretHttpPolicy::LegacyHosts(HashSet::from(["api.stashbase.dev".to_owned()])),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::from(["api.stashbase.dev".to_owned()]),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: true,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };
        let state = ProxyState {
            secrets: Arc::new(HashMap::new()),
            policy,
            client: reqwest::Client::new(),
            remote_ca: None,
            certificate_authority: Arc::new(
                CertifiedIssuer::self_signed(
                    CertificateParams::default(),
                    KeyPair::generate().unwrap(),
                )
                .unwrap(),
            ),
            audit_log: None,
            connections: Arc::new(ActiveConnections::default()),
            remote: None,
            mcp_inspection_token: "inspection-token".to_owned(),
            hook_broker: None,
            revocation_path: Arc::new(RwLock::new(None)),
        };

        assert!(state.host_allowed_for_connect(Some("chatgpt.com")));
        assert!(!state.host_allowed_for_connect(Some("api.stashbase.dev")));
    }

    #[tokio::test]
    async fn rewrites_a_placeholder_before_forwarding_a_http_request() {
        let (address, authorization) = start_backend().await;
        let proxy = Proxy::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            ProxyPolicy::permissive(),
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/"))
            .header(AUTHORIZATION, "Bearer **STASHBASE_GH_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            authorization.await.unwrap().as_deref(),
            Some("Bearer real-token")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn rewrites_a_placeholder_when_the_binding_is_renamed_via_env() {
        // Mirrors an agent profile binding `[secrets.GH_TOKEN]` with
        // `env = "GITHUB_PAT_TOKEN"`: the secret is fetched under the
        // source name `GH_TOKEN`, but both the child-visible env var and
        // the policy/secret map are keyed by the renamed target name, the
        // same way `secret_child_name` renames both in root.rs.
        let (address, authorization) = start_backend().await;
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "GITHUB_PAT_TOKEN".to_owned(),
                SecretHttpPolicy::LegacyHosts(HashSet::from(["127.0.0.1".to_owned()])),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: true,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };
        let proxy = Proxy::start(
            HashMap::from([("GITHUB_PAT_TOKEN".to_owned(), "real-token".to_owned())]),
            policy,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            proxy
                .child_env()
                .get("GITHUB_PAT_TOKEN")
                .map(String::as_str),
            Some("**STASHBASE_GITHUB_PAT_TOKEN**")
        );
        assert!(!proxy.child_env().contains_key("GH_TOKEN"));

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/"))
            .header(AUTHORIZATION, "Bearer **STASHBASE_GITHUB_PAT_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            authorization.await.unwrap().as_deref(),
            Some("Bearer real-token")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn secret_hosts_do_not_grant_ordinary_egress() {
        let (address, authorization) = start_backend().await;
        let policy = ProxyPolicy {
            secret_policies: HashMap::from([(
                "GH_TOKEN".to_owned(),
                SecretHttpPolicy::LegacyHosts(HashSet::from(["127.0.0.1".to_owned()])),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            denied_read_paths: Vec::new(),
            denied_write_paths: Vec::new(),
            allow_network_listeners: false,
            egress_hosts_configured: false,
            strict_deny: true,
            mcp_rules: Vec::new(),
            backend: SandboxBackend::Native,
            sandbox_image: None,
            sandbox_dockerfile: None,
            sandbox_memory: None,
            sandbox_cpus: None,
            worktree: false,
            worktree_resume: None,
            sandbox_isolated_paths: Vec::new(),
        };
        let proxy = Proxy::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            policy,
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(timeout(Duration::from_millis(100), authorization)
            .await
            .is_err());

        proxy.stop().await;
    }

    #[tokio::test]
    async fn tunnels_an_http_upgrade_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(|mut request: Request<Incoming>| async move {
                let upgraded = hyper::upgrade::on(&mut request);
                tokio::spawn(async move {
                    let upgraded = upgraded.await.unwrap();
                    let mut stream = TokioIo::new(upgraded);
                    let mut received = [0; 4];
                    stream.read_exact(&mut received).await.unwrap();
                    assert_eq!(&received, b"ping");
                    stream.write_all(b"pong").await.unwrap();
                });
                Ok::<_, Infallible>(
                    Response::builder()
                        .status(StatusCode::SWITCHING_PROTOCOLS)
                        .header("connection", "upgrade")
                        .header("upgrade", "websocket")
                        .body(Full::new(Bytes::new()))
                        .unwrap(),
                )
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await
                .unwrap();
        });

        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();
        let proxy_address = proxy.child_env()["HTTP_PROXY"].trim_start_matches("http://");
        let stream = TcpStream::connect(proxy_address).await.unwrap();
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream)).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.with_upgrades().await;
        });
        let request = Request::builder()
            .uri(format!("http://{backend}/ws"))
            .header("connection", "upgrade")
            .header("upgrade", "websocket")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let mut response = sender.send_request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

        let mut stream = TokioIo::new(hyper::upgrade::on(&mut response).await.unwrap());
        stream.write_all(b"ping").await.unwrap();
        let mut response = [0; 4];
        stream.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");
        proxy.stop().await;
    }

    #[tokio::test]
    async fn forwards_a_json_request_body_without_modifying_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (body_sender, body) = oneshot::channel();
        let body_sender = Arc::new(Mutex::new(Some(body_sender)));
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: Request<Incoming>| {
                let body_sender = body_sender.clone();
                async move {
                    let bytes = request.into_body().collect().await.unwrap().to_bytes();
                    if let Some(sender) = body_sender.lock().unwrap().take() {
                        let _ = sender.send(bytes);
                    }
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();

        let payload = r#"{"model":"example","stream":true}"#;
        let response = proxy_client(&proxy)
            .post(format!("http://{address}/v1/chat"))
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body.await.unwrap(), Bytes::from(payload));
        proxy.stop().await;
    }

    #[tokio::test]
    async fn streams_request_chunks_to_the_upstream_before_the_body_completes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (first_chunk_sender, first_chunk) = oneshot::channel();
        let first_chunk_sender = Arc::new(Mutex::new(Some(first_chunk_sender)));
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: Request<Incoming>| {
                let first_chunk_sender = first_chunk_sender.clone();
                async move {
                    let transfer_encoding = request
                        .headers()
                        .get(TRANSFER_ENCODING)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    let mut body = request.into_body().into_data_stream();
                    if let Some(Ok(chunk)) = body.next().await {
                        if let Some(sender) = first_chunk_sender.lock().unwrap().take() {
                            let _ = sender.send((transfer_encoding, chunk));
                        }
                    }
                    while body.next().await.is_some() {}
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
                }
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();
        let client = proxy_client(&proxy);
        let body = stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first"))]).chain(
            stream::once(async {
                sleep(Duration::from_millis(200)).await;
                Ok(Bytes::from_static(b"second"))
            }),
        );
        let request = tokio::spawn(async move {
            client
                .post(format!("http://{address}/upload"))
                .body(reqwest::Body::wrap_stream(body))
                .send()
                .await
                .unwrap()
        });

        let (transfer_encoding, first_chunk) = timeout(Duration::from_millis(100), first_chunk)
            .await
            .expect("the first chunk was buffered by the proxy")
            .unwrap();
        assert_eq!(transfer_encoding.as_deref(), Some("chunked"));
        assert_eq!(first_chunk, Bytes::from_static(b"first"));
        assert_eq!(request.await.unwrap().status(), StatusCode::OK);
        proxy.stop().await;
    }

    #[tokio::test]
    async fn streams_sse_response_chunks_without_waiting_for_completion() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(|_: Request<Incoming>| async move {
                let events = stream::iter([Ok::<Bytes, Infallible>(Bytes::from_static(
                    b"data: first\n\n",
                ))])
                .chain(stream::once(async {
                    sleep(Duration::from_millis(200)).await;
                    Ok(Bytes::from_static(b"data: second\n\n"))
                }))
                .map(|chunk| chunk.map(Frame::data));
                Ok::<_, Infallible>(
                    Response::builder()
                        .header(CONTENT_TYPE, "text/event-stream")
                        .body(StreamBody::new(events))
                        .unwrap(),
                )
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/events"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers()[CONTENT_TYPE], "text/event-stream");
        assert_eq!(response.headers()[TRANSFER_ENCODING], "chunked");
        let mut events = response.bytes_stream();
        assert_eq!(
            timeout(Duration::from_millis(100), events.next())
                .await
                .expect("the first SSE event was buffered by the proxy")
                .unwrap()
                .unwrap(),
            Bytes::from_static(b"data: first\n\n")
        );
        assert_eq!(
            events.next().await.unwrap().unwrap(),
            Bytes::from_static(b"data: second\n\n")
        );
        proxy.stop().await;
    }

    #[tokio::test]
    async fn streams_large_request_and_response_bodies() {
        const CHUNK_SIZE: usize = 128 * 1024;
        const CHUNKS: usize = 64;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(|request: Request<Incoming>| async move {
                let mut request_body = request.into_body().into_data_stream();
                let mut request_size = 0;
                while let Some(chunk) = request_body.next().await {
                    request_size += chunk.unwrap().len();
                }
                assert_eq!(request_size, CHUNK_SIZE * CHUNKS);
                let response = stream::unfold(0, |index| async move {
                    (index < CHUNKS).then(|| {
                        (
                            Ok::<Bytes, Infallible>(Bytes::from(vec![b'r'; CHUNK_SIZE])),
                            index + 1,
                        )
                    })
                })
                .map(|chunk| chunk.map(Frame::data));
                Ok::<_, Infallible>(Response::new(StreamBody::new(response)))
            });
            http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
        });
        let proxy = Proxy::start(HashMap::new(), ProxyPolicy::permissive(), None)
            .await
            .unwrap();
        let request = stream::unfold(0, |index| async move {
            (index < CHUNKS).then(|| {
                (
                    Ok::<Bytes, std::io::Error>(Bytes::from(vec![b'q'; CHUNK_SIZE])),
                    index + 1,
                )
            })
        });

        let mut response = proxy_client(&proxy)
            .post(format!("http://{address}/large"))
            .body(reqwest::Body::wrap_stream(request))
            .send()
            .await
            .unwrap()
            .bytes_stream();
        let mut response_size = 0;
        while let Some(chunk) = response.next().await {
            response_size += chunk.unwrap().len();
        }
        assert_eq!(response_size, CHUNK_SIZE * CHUNKS);
        proxy.stop().await;
    }

    #[tokio::test]
    async fn rejects_and_audits_an_unknown_placeholder_without_recording_it() {
        let path =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}.jsonl", Uuid::new_v4()));
        let audit_log = ProxyAuditLog {
            session_id: "session".to_owned(),
            profile: "coding".to_owned(),
            policy_fingerprint: "policy-fingerprint".to_owned(),
            profile_provenance: None,
            routing: None,
            binding_sources: Arc::new(HashMap::new()),
            path: Arc::new(path.clone()),
            file: Arc::new(Mutex::new(
                OpenOptions::new()
                    .create_new(true)
                    .append(true)
                    .open(&path)
                    .unwrap(),
            )),
        };
        let proxy = Proxy::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            ProxyPolicy::permissive(),
            Some(audit_log),
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get("http://127.0.0.1:1/")
            .header(AUTHORIZATION, "Bearer **STASHBASE_STALE_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        proxy.stop().await;

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("unknown_placeholder"));
        assert!(!content.contains("STASHBASE_STALE_TOKEN"));
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn rewrites_a_placeholder_in_a_configured_api_key_header() {
        let header_name = HeaderName::from_static("x-api-key");
        let (address, api_key) = start_backend_capturing(header_name.clone()).await;
        let proxy = Proxy::start(
            HashMap::from([("ANTHROPIC_API_KEY".to_owned(), "real-token".to_owned())]),
            ProxyPolicy {
                secret_policies: HashMap::from([(
                    "ANTHROPIC_API_KEY".to_owned(),
                    SecretHttpPolicy::LegacyHosts(HashSet::from(["127.0.0.1".to_owned()])),
                )]),
                secret_injections: HashMap::from([(
                    "ANTHROPIC_API_KEY".to_owned(),
                    SecretInjection {
                        header: header_name.to_string(),
                        value_template: "{value}".to_owned(),
                    },
                )]),
                allowed_egress_hosts: HashSet::new(),
                denied_hosts: HashSet::new(),
                denied_read_paths: Vec::new(),
                denied_write_paths: Vec::new(),
                allow_network_listeners: false,
                egress_hosts_configured: false,
                strict_deny: true,
                mcp_rules: Vec::new(),
                backend: SandboxBackend::Native,
                sandbox_image: None,
                sandbox_dockerfile: None,
                sandbox_memory: None,
                sandbox_cpus: None,
                worktree: false,
                worktree_resume: None,
                sandbox_isolated_paths: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/"))
            .header("x-api-key", "**STASHBASE_ANTHROPIC_API_KEY**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(api_key.await.unwrap().as_deref(), Some("real-token"));
        proxy.stop().await;
    }

    #[tokio::test]
    async fn strict_policy_denies_unapproved_destinations_and_sets_node_environment() {
        let (address, _authorization) = start_backend().await;
        let proxy = Proxy::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            ProxyPolicy {
                secret_policies: HashMap::from([(
                    "GH_TOKEN".to_owned(),
                    SecretHttpPolicy::LegacyHosts(HashSet::from(["api.github.com".to_owned()])),
                )]),
                secret_injections: HashMap::new(),
                allowed_egress_hosts: HashSet::new(),
                denied_hosts: HashSet::new(),
                denied_read_paths: Vec::new(),
                denied_write_paths: Vec::new(),
                allow_network_listeners: false,
                egress_hosts_configured: false,
                strict_deny: true,
                mcp_rules: Vec::new(),
                backend: SandboxBackend::Native,
                sandbox_image: None,
                sandbox_dockerfile: None,
                sandbox_memory: None,
                sandbox_cpus: None,
                worktree: false,
                worktree_resume: None,
                sandbox_isolated_paths: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/"))
            .header(AUTHORIZATION, "Bearer **STASHBASE_GH_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(proxy.child_env()["NODE_USE_ENV_PROXY"], "1");
        assert!(std::path::Path::new(&proxy.child_env()["NODE_EXTRA_CA_CERTS"]).exists());
        proxy.stop().await;
    }

    #[tokio::test]
    async fn egress_only_host_is_forwarded_without_credential_injection() {
        let (address, authorization) = start_backend().await;
        let proxy = Proxy::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            ProxyPolicy {
                secret_policies: HashMap::from([(
                    "GH_TOKEN".to_owned(),
                    SecretHttpPolicy::LegacyHosts(HashSet::from(["api.github.com".to_owned()])),
                )]),
                secret_injections: HashMap::new(),
                allowed_egress_hosts: HashSet::from(["127.0.0.1".to_owned()]),
                denied_hosts: HashSet::new(),
                denied_read_paths: Vec::new(),
                denied_write_paths: Vec::new(),
                allow_network_listeners: false,
                egress_hosts_configured: true,
                strict_deny: true,
                mcp_rules: Vec::new(),
                backend: SandboxBackend::Native,
                sandbox_image: None,
                sandbox_dockerfile: None,
                sandbox_memory: None,
                sandbox_cpus: None,
                worktree: false,
                worktree_resume: None,
                sandbox_isolated_paths: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&proxy)
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(authorization.await.unwrap(), None);
        proxy.stop().await;
    }
}
