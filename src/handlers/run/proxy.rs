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
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{Bytes, Frame, Incoming},
    client::conn::http1 as client_http1,
    header::{HeaderName, HeaderValue, CONTENT_TYPE},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use log::debug;
use rcgen::{BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyUsagePurpose};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    ClientConfig, ServerConfig,
};
use rustls_platform_verifier::BuilderVerifierExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use uuid::Uuid;

use crate::{
    models::agent::{AgentHttpRule, AgentHttpRuleEffect},
    REQUEST_TIMEOUT_SECS,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;
type ProxyFuture = Pin<Box<dyn Future<Output = Result<Response<ProxyBody>, Infallible>> + Send>>;

const AUDIT_LOG_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUDIT_LOG_MAX_FILES: usize = 1_000;

/// One metadata-only event emitted by the local proxy audit log.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ProxyAuditLogEvent {
    pub timestamp: String,
    pub session_id: String,
    pub profile: String,
    pub action: String,
    pub destination_host: Option<String>,
    pub method: Option<String>,
    pub secret_name: Option<String>,
    pub response_status: Option<u16>,
    pub duration_ms: Option<u64>,
}

/// Exact-match filters for the local audit-log viewer.
#[derive(Debug, Clone, Default)]
pub struct ProxyAuditLogFilter {
    pub profile: Option<String>,
    pub action: Option<String>,
    pub host: Option<String>,
    pub session: Option<String>,
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
    }
}

/// Private, metadata-only audit log for one proxy session.
#[derive(Debug, Clone)]
pub struct ProxyAuditLog {
    session_id: String,
    profile: String,
    path: Arc<PathBuf>,
    file: Arc<Mutex<std::fs::File>>,
}

impl ProxyAuditLog {
    pub fn local(profile: &str) -> Result<Self> {
        Self::local_with_session_id(profile, Uuid::new_v4().to_string())
    }

    /// Uses the control-plane session identifier so local metadata can be
    /// correlated with future server-side remote-proxy audit events.
    pub fn local_with_session_id(profile: &str, session_id: String) -> Result<Self> {
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

    fn record(
        &self,
        action: &str,
        host: Option<&str>,
        method: Option<&Method>,
        secret_name: Option<&str>,
        status: Option<StatusCode>,
        duration: Option<Duration>,
    ) {
        let event = ProxyAuditLogEvent {
            timestamp: Utc::now().to_rfc3339(),
            session_id: self.session_id.clone(),
            profile: self.profile.clone(),
            action: action.to_owned(),
            destination_host: host.map(str::to_owned),
            method: method.map(Method::as_str).map(str::to_owned),
            secret_name: secret_name.map(str::to_owned),
            response_status: status.map(|status| status.as_u16()),
            duration_ms: duration.and_then(|duration| u64::try_from(duration.as_millis()).ok()),
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
    /// Whether credential-bearing requests must also satisfy egress policy.
    pub egress_hosts_configured: bool,
    pub strict_deny: bool,
}

/// Authorization for one credential. A binding is deliberately either legacy
/// host-only policy or HTTP rules, never both.
#[derive(Debug, Clone)]
pub enum SecretHttpPolicy {
    LegacyHosts(HashSet<String>),
    Rules(Vec<AgentHttpRule>),
}

/// The credential decision for one request, without reading or exposing a
/// secret value. Used by both the proxy and `agent explain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretAuthorizationDecision {
    AllowedLegacyHost,
    AllowedRule,
    DeniedLegacyHost,
    DeniedRule,
    NoMatchingAllowRule,
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
            value_template: "Bearer {secret}".to_owned(),
        }
    }
}

impl ProxyPolicy {
    pub fn permissive() -> Self {
        Self {
            secret_policies: HashMap::new(),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            egress_hosts_configured: false,
            strict_deny: false,
        }
    }
}

#[derive(Clone)]
struct ProxyState {
    secrets: Arc<HashMap<String, String>>,
    policy: ProxyPolicy,
    client: reqwest::Client,
    remote_ca: Option<reqwest::Certificate>,
    certificate_authority: Arc<Certificate>,
    audit_log: Option<ProxyAuditLog>,
    connections: Arc<ActiveConnections>,
    remote: Option<RemoteProxyConfig>,
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
        Self::start_inner(secrets, policy, audit_log, proxy_port, None).await
    }

    pub async fn start_remote_with_port(
        remote: RemoteProxyConfig,
        policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
    ) -> Result<Self> {
        let placeholders = remote.placeholders.clone();
        Self::start_inner(placeholders, policy, audit_log, proxy_port, Some(remote)).await
    }

    async fn start_inner(
        secrets: HashMap<String, String>,
        mut policy: ProxyPolicy,
        audit_log: Option<ProxyAuditLog>,
        proxy_port: Option<u16>,
        remote: Option<RemoteProxyConfig>,
    ) -> Result<Self> {
        if proxy_port == Some(0) {
            anyhow::bail!("--proxy-port must be between 1 and 65535");
        }
        let (certificate_authority, mut ca_file) = create_certificate_authority()?;
        let mut remove_ca_file = true;
        if remote
            .as_ref()
            .is_some_and(|remote| remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept)
        {
            let remote_ca = remote
                .as_ref()
                .and_then(|remote| remote.ca_file.clone())
                .context("remote forward-proxy session did not provide a cached CA file")?;
            // The remote listener, not this local relay, presents certificates in
            // forward-proxy mode. Pass its public CA to the child.
            ca_file = remote_ca;
            remove_ca_file = false;
        }
        let bind_address = format!("127.0.0.1:{}", proxy_port.unwrap_or(0));
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
                let secret_policy = match secret_policy {
                    SecretHttpPolicy::LegacyHosts(hosts) => {
                        SecretHttpPolicy::LegacyHosts(normalize_hosts(hosts))
                    }
                    SecretHttpPolicy::Rules(rules) => {
                        SecretHttpPolicy::Rules(normalize_http_rules(rules))
                    }
                };
                (placeholder, secret_policy)
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
        let remote_ca = if remote
            .as_ref()
            .is_some_and(|remote| remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept)
        {
            let remote_ca = reqwest::Certificate::from_pem(&fs::read(&ca_file)?)?;
            client_builder = client_builder.add_root_certificate(remote_ca.clone());
            Some(remote_ca)
        } else {
            None
        };
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
        ]);
        for placeholder in state.secrets.keys() {
            let env_name = child_env_name_for_placeholder(
                placeholder,
                remote_binding_names.as_ref(),
                remote_child_env.as_ref(),
            );
            child_env.insert(env_name, placeholder.clone());
        }

        if let Some(audit_log) = &audit_log {
            audit_log.record("session_started", None, None, None, None, None);
        }

        Ok(Self {
            child_env,
            shutdown: Some(shutdown),
            task: Some(task),
            ca_file,
            remove_ca_file,
            audit_log,
            connections,
        })
    }

    pub fn child_env(&self) -> &HashMap<String, String> {
        &self.child_env
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
            audit_log.record("session_stopped", None, None, None, None, None);
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

fn create_certificate_authority() -> Result<(Certificate, PathBuf)> {
    let subject = format!("Stashbase Proxy {}", Uuid::new_v4());
    let mut params = CertificateParams::new(vec!["stashbase-proxy.local".to_owned()]);
    params
        .distinguished_name
        .push(DnType::CommonName, subject.clone());
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let ca = Certificate::from_params(params)?;
    let path = std::env::temp_dir().join(format!("stashbase-proxy-ca-{}.pem", Uuid::new_v4()));
    std::fs::write(&path, ca.serialize_pem()?).context("failed to write temporary proxy CA")?;
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
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
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
                    "proxy.host_denied",
                    "Agent Proxy policy denied destination",
                ));
            }
            if let Some(remote) = &state.remote {
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
            let remote_tunnel =
                match state.remote.clone().filter(|remote| {
                    remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept
                }) {
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

        let host = request_host(&request, connect_authority.as_deref());
        if state.host_is_denied(host.as_deref()) {
            debug!(
                "proxy denied destination: {}",
                host.as_deref().unwrap_or("unknown")
            );
            state.record_audit(
                "host_denied",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response(
                StatusCode::FORBIDDEN,
                "proxy.host_denied",
                "Agent Proxy policy denied destination",
            ));
        }
        if state.policy.egress_hosts_configured
            && !host
                .as_deref()
                .is_some_and(|host| policy_allows_egress(&state.policy, host))
        {
            state.record_audit(
                "host_denied",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response(
                StatusCode::FORBIDDEN,
                "proxy.host_denied",
                "Agent Proxy policy denied destination",
            ));
        }
        if contains_unknown_placeholder(&request, &state) {
            state.record_audit(
                "unknown_placeholder",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response(
                StatusCode::FORBIDDEN,
                "proxy.unknown_placeholder",
                "Agent Proxy received an unknown credential placeholder",
            ));
        }
        let secret_name = match replace_placeholder(&mut request, &state, host.as_deref()) {
            Ok(secret_name) => secret_name,
            Err(denial) => {
                debug!(
                    "proxy denied credential injection for destination: {}",
                    host.as_deref().unwrap_or("unknown")
                );
                state.record_audit(
                    denial.audit_action,
                    host.as_deref(),
                    Some(request.method()),
                    Some(&denial.secret_name),
                    Some(StatusCode::FORBIDDEN),
                    Some(started.elapsed()),
                );
                return Ok(proxy_error_response(
                    StatusCode::FORBIDDEN,
                    "proxy.credential_not_allowed",
                    "The supplied credential is not authorized for this request.",
                ));
            }
        };
        // A per-secret host is not general egress. It is permitted only when
        // this request carries that secret's configured placeholder; all other
        // traffic must be explicitly listed in `egress_hosts`.
        if secret_name.is_none() && !state.host_allowed_for_ordinary_request(host.as_deref()) {
            state.record_audit(
                "host_denied",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(proxy_error_response(
                StatusCode::FORBIDDEN,
                "proxy.host_denied",
                "Agent Proxy policy denied destination",
            ));
        }
        if let Some(remote) = &state.remote {
            if let Err(error) = remote.token_for_new_connection() {
                state.record_audit(
                    "session_expired",
                    host.as_deref(),
                    Some(request.method()),
                    secret_name.as_deref(),
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
        // Reqwest deliberately does not support HTTP upgrade responses. Coding agents
        // such as Codex use a WSS connection for streaming, so tunnel an upgraded
        // connection after applying the same destination and placeholder checks.
        if is_upgrade_request(&request) {
            if let Some(remote) = state.remote.clone() {
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
                state.record_audit(
                    "request_invalid",
                    host.as_deref(),
                    Some(request.method()),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_REQUEST),
                    Some(started.elapsed()),
                );
                return Ok(proxy_error_response(
                    StatusCode::BAD_REQUEST,
                    "proxy.request_invalid",
                    "Unable to determine request URL",
                ));
            }
        };
        let method = request.method().clone();
        let mut headers = request.headers().clone();
        // `Incoming` is converted into a data stream without collecting it. Reqwest
        // applies chunked transfer encoding when no content length is available, so
        // streaming uploads retain their incremental delivery to the upstream.
        let body = reqwest::Body::wrap_stream(request.into_body().into_data_stream());

        let destination_url = if let Some(remote) = state
            .remote
            .as_ref()
            .filter(|remote| remote.protocol == RemoteProxyProtocol::Custom)
        {
            headers.remove("x-stashbase-target");
            headers.remove("x-stashbase-session");
            let token = match remote.token_for_new_connection() {
                Ok(token) => token,
                Err(error) => {
                    return Ok(proxy_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "proxy.session_expired",
                        &error.to_string(),
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
                    return Ok(proxy_error_response(
                        StatusCode::BAD_GATEWAY,
                        "proxy.session_invalid",
                        "Agent Proxy returned an invalid session token",
                    ))
                }
            };
            headers.insert("x-stashbase-session", token);
            let proxy = match reqwest::Url::parse(&remote.proxy_url) {
                Ok(proxy) => proxy,
                Err(_) => {
                    return Ok(proxy_error_response(
                        StatusCode::BAD_GATEWAY,
                        "proxy.session_invalid",
                        "Agent Proxy returned an invalid proxy URL",
                    ))
                }
            };
            // This is a forward request to the remote proxy, not the original
            // destination. Keeping the child's Host header sends the wrong virtual
            // host for ordinary (non-upgrade) requests.
            let proxy_host = match proxy_host_header(&proxy) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(proxy_error_response(
                        StatusCode::BAD_GATEWAY,
                        "proxy.session_invalid",
                        "Agent Proxy returned an invalid proxy URL",
                    ))
                }
            };
            headers.insert(hyper::header::HOST, proxy_host);
            remote.proxy_url.clone()
        } else {
            url.to_string()
        };
        let client = match state
            .remote
            .as_ref()
            .filter(|remote| remote.protocol == RemoteProxyProtocol::ForwardProxyTlsIntercept)
        {
            Some(remote) => match remote_forward_client(remote, state.remote_ca.as_ref()) {
                Ok(client) => client,
                Err(error) => {
                    return Ok(proxy_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "proxy.session_unavailable",
                        &error.to_string(),
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
                let headers = upstream.headers().clone();
                // Do not await `bytes()`: forwarding this stream lets clients observe
                // each upstream chunk (including SSE events) as it arrives.
                let body = StreamBody::new(upstream.bytes_stream().map(|chunk| {
                    chunk
                        .map(Frame::data)
                        .map_err(|error| -> BoxError { Box::new(error) })
                }))
                .boxed_unsync();
                let mut response = Response::builder().status(status).body(body).unwrap();
                *response.headers_mut() = headers;
                state.record_audit(
                    if secret_name.is_some() {
                        "injected"
                    } else {
                        "forwarded"
                    },
                    host.as_deref(),
                    Some(&method),
                    secret_name.as_deref(),
                    Some(status),
                    Some(started.elapsed()),
                );
                Ok(response)
            }
            Err(error) => {
                debug!(
                    "proxy could not forward request to destination: {}",
                    host.as_deref().unwrap_or("unknown")
                );
                state.record_audit(
                    upstream_error_action(&error),
                    host.as_deref(),
                    Some(&method),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_GATEWAY),
                    Some(started.elapsed()),
                );
                Ok(proxy_error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("proxy.{}", upstream_error_action(&error)),
                    "Unable to forward Agent Proxy request",
                ))
            }
        }
    })
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
    let mut params = CertificateParams::new(vec![host.to_owned()]);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let leaf = Certificate::from_params(params)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(
                leaf.serialize_der_with_signer(&state.certificate_authority)?,
            )],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf.serialize_private_key_der())),
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
            audit_log.record(action, host, method, secret_name, status, duration);
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
        let expected = injection.value_template.replace("{secret}", placeholder);
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
        let value = injection.value_template.replace("{secret}", secret);
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

pub fn evaluate_secret_authorization(
    policy: &SecretHttpPolicy,
    host: &str,
    method: &str,
    path: &str,
) -> SecretAuthorizationDecision {
    match policy {
        SecretHttpPolicy::LegacyHosts(hosts) => {
            if hosts
                .iter()
                .any(|allowed| configured_host_matches(allowed, host))
            {
                SecretAuthorizationDecision::AllowedLegacyHost
            } else {
                SecretAuthorizationDecision::DeniedLegacyHost
            }
        }
        SecretHttpPolicy::Rules(rules) => {
            let path = normalize_request_path(path);
            let matches = |rule: &AgentHttpRule| {
                rule.hosts
                    .iter()
                    .any(|allowed| configured_host_matches(allowed, host))
                    && rule
                        .methods
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(method))
                    && rule
                        .paths
                        .iter()
                        .any(|pattern| path_matches(&normalize_path_pattern(pattern), &path))
            };
            if rules
                .iter()
                .any(|rule| rule.effect == AgentHttpRuleEffect::Deny && matches(rule))
            {
                SecretAuthorizationDecision::DeniedRule
            } else if rules
                .iter()
                .any(|rule| rule.effect == AgentHttpRuleEffect::Allow && matches(rule))
            {
                SecretAuthorizationDecision::AllowedRule
            } else {
                SecretAuthorizationDecision::NoMatchingAllowRule
            }
        }
    }
}

fn normalize_http_rules(rules: Vec<AgentHttpRule>) -> Vec<AgentHttpRule> {
    rules
        .into_iter()
        .map(|mut rule| {
            rule.hosts = rule
                .hosts
                .into_iter()
                .map(|host| host.trim().trim_end_matches('.').to_ascii_lowercase())
                .collect();
            rule.methods = rule
                .methods
                .into_iter()
                .map(|method| method.trim().to_ascii_uppercase())
                .collect();
            rule.paths = rule
                .paths
                .into_iter()
                .map(|path| normalize_path_pattern(&path))
                .collect();
            rule
        })
        .collect()
}

fn normalize_path_pattern(pattern: &str) -> String {
    if pattern == "*" {
        return "*".to_owned();
    }
    normalize_path_segments(pattern)
}

fn normalize_request_path(path: &str) -> String {
    normalize_path_segments(path)
}

fn normalize_path_segments(path: &str) -> String {
    let mut segments = Vec::new();
    let path = path
        .replace("%2f", "/")
        .replace("%2F", "/")
        .replace("%5c", "/")
        .replace("%5C", "/")
        .replace('\\', "/");
    for segment in path.split('/') {
        let segment = segment.replace("%2e", ".").replace("%2E", ".");
        match segment.as_str() {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    format!("/{}", segments.join("/"))
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let mut remainder = path;
    let mut first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        if first {
            if !remainder.starts_with(part) {
                return false;
            }
            remainder = &remainder[part.len()..];
            first = false;
        } else if let Some(index) = remainder.find(part) {
            remainder = &remainder[index + part.len()..];
        } else {
            return false;
        }
    }
    pattern.ends_with('*') || remainder.is_empty()
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
            if !injection.value_template.contains("{secret}") {
                anyhow::bail!(
                    "credential value template for secret '{name}' must contain '{{secret}}'"
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

fn host_matches(allowed: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match allowed.strip_prefix("*.") {
        Some(suffix) => host != suffix && host.ends_with(&format!(".{suffix}")),
        None => allowed == host,
    }
}

pub fn configured_host_matches(allowed: &str, host: &str) -> bool {
    host_matches(
        &allowed.trim().trim_end_matches('.').to_ascii_lowercase(),
        host,
    )
}

/// Proxy failures use the public API error envelope so a nested `stashbase`
/// command can report policy denials clearly instead of failing JSON parsing.
fn proxy_error_response(status: StatusCode, code: &str, message: &str) -> Response<ProxyBody> {
    let body = serde_json::json!({
        "error": {
            "code": code,
            "message": message,
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
    use hyper::header::{AUTHORIZATION, HOST, TRANSFER_ENCODING};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::oneshot,
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
                value_template: "{secret}".to_owned(),
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
                    value_template: "{secret}".to_owned(),
                },
            )]),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::new(),
            egress_hosts_configured: true,
            strict_deny: true,
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
        };

        let error = match establish_remote_connect("api.example.com:443", &remote).await {
            Ok(_) => panic!("rejected remote CONNECT should not open a tunnel"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("rejected CONNECT"));
    }

    #[test]
    fn creates_expected_placeholders() {
        assert_eq!(placeholder_for("GH_TOKEN"), "**STASHBASE_GH_TOKEN**");
    }

    #[tokio::test]
    async fn unclosed_dollar_brace_in_header_is_not_treated_as_unknown_placeholder() {
        // An unclosed "${" (shell template, JS interpolation, etc.) must not
        // produce a false-positive 403 proxy.unknown_placeholder response.
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
    async fn proxy_errors_use_the_api_error_envelope() {
        let response = proxy_error_response(
            StatusCode::FORBIDDEN,
            "proxy.host_denied",
            "Agent Proxy policy denied destination",
        );
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let error: crate::models::api_client::ApiErrorResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(error.error.code, "proxy.host_denied");
        assert_eq!(
            error.error.message.as_deref(),
            Some("Agent Proxy policy denied destination")
        );
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
        );

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("api.example.com"));
        assert!(content.contains("EXAMPLE_API_KEY"));
        assert!(!content.contains("real-secret-value"));
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
            action: "injected".to_owned(),
            destination_host: Some("api.github.com".to_owned()),
            method: Some("POST".to_owned()),
            secret_name: Some("GH_TOKEN".to_owned()),
            response_status: Some(200),
            duration_ms: Some(42),
        };

        assert!(ProxyAuditLogFilter {
            profile: Some("coding".to_owned()),
            action: Some("injected".to_owned()),
            host: Some("api.github.com".to_owned()),
            session: Some("session-1".to_owned()),
        }
        .matches(&event));
        assert!(!ProxyAuditLogFilter {
            host: Some("example.com".to_owned()),
            ..Default::default()
        }
        .matches(&event));
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
            egress_hosts_configured: false,
            strict_deny: true,
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
            egress_hosts_configured: false,
            strict_deny: true,
        }
    }

    #[test]
    fn http_rules_allow_a_matching_request() {
        let policy = rule_policy(normalize_http_rules(vec![rule(
            AgentHttpRuleEffect::Allow,
            &["get"],
            &["/repos/*"],
        )]));
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
        let policy = rule_policy(normalize_http_rules(vec![rule(
            AgentHttpRuleEffect::Allow,
            &["GET"],
            &["/repos/*"],
        )]));
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
        let policy = rule_policy(normalize_http_rules(vec![
            rule(AgentHttpRuleEffect::Allow, &["GET"], &["/repos/*"]),
            rule(AgentHttpRuleEffect::Deny, &["GET"], &["/repos/private/*"]),
        ]));
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
            egress_hosts_configured: false,
            strict_deny: true,
        };
        assert!(secret_allows_request(
            &policy,
            "**STASHBASE_GH_TOKEN**",
            Some("api.github.com"),
            &Method::DELETE,
            "/anything"
        ));
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
            egress_hosts_configured: true,
            strict_deny: true,
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
            egress_hosts_configured: true,
            strict_deny: true,
        };
        let state = ProxyState {
            secrets: Arc::new(HashMap::new()),
            policy,
            client: reqwest::Client::new(),
            remote_ca: None,
            certificate_authority: Arc::new(
                Certificate::from_params(CertificateParams::default()).unwrap(),
            ),
            audit_log: None,
            connections: Arc::new(ActiveConnections::default()),
            remote: None,
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
            egress_hosts_configured: false,
            strict_deny: true,
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
                        value_template: "{secret}".to_owned(),
                    },
                )]),
                allowed_egress_hosts: HashSet::new(),
                denied_hosts: HashSet::new(),
                egress_hosts_configured: false,
                strict_deny: true,
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
                egress_hosts_configured: false,
                strict_deny: true,
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
                egress_hosts_configured: true,
                strict_deny: true,
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
