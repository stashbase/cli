//! Embedded, per-command credential broker for `stashbase run --broker` and
//! `stashbase agent run`.
//!
//! The broker binds only to localhost and lives for the child process lifetime.
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
    sync::{Arc, Mutex},
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
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use uuid::Uuid;

use crate::REQUEST_TIMEOUT_SECS;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;
type ProxyFuture = Pin<Box<dyn Future<Output = Result<Response<ProxyBody>, Infallible>> + Send>>;

const AUDIT_LOG_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUDIT_LOG_MAX_FILES: usize = 1_000;

/// One metadata-only event emitted by the local broker audit log.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct AuditLogEvent {
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
pub struct AuditLogFilter {
    pub profile: Option<String>,
    pub action: Option<String>,
    pub host: Option<String>,
    pub session: Option<String>,
}

impl AuditLogFilter {
    fn matches(&self, event: &AuditLogEvent) -> bool {
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

/// Private, metadata-only audit log for one broker session.
#[derive(Debug, Clone)]
pub struct AuditLog {
    session_id: String,
    profile: String,
    path: Arc<PathBuf>,
    file: Arc<Mutex<std::fs::File>>,
}

impl AuditLog {
    pub fn local(profile: &str) -> Result<Self> {
        let directory = audit_directory()?;
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        prune_audit_logs(&directory)?;

        let session_id = Uuid::new_v4().to_string();
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
        let event = AuditLogEvent {
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
pub fn read_local_audit_logs(
    limit: usize,
    since: Option<Duration>,
    filter: &AuditLogFilter,
) -> Result<Vec<AuditLogEvent>> {
    let directory = audit_directory()?;
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
            let Ok(event) = serde_json::from_str::<AuditLogEvent>(line) else {
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

fn audit_directory() -> Result<PathBuf> {
    let config_path = crate::config::config::get_config_path()?;
    Ok(config_path
        .parent()
        .context("Stashbase config path has no parent directory")?
        .join("audit"))
}

/// Keeps local audit storage bounded without touching files outside our session naming scheme.
fn prune_audit_logs(directory: &Path) -> Result<()> {
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

/// Destination policy for credentials brokered into an agent process.
#[derive(Debug, Clone)]
pub struct BrokerPolicy {
    pub allowed_hosts_by_secret: HashMap<String, HashSet<String>>,
    pub secret_injections: HashMap<String, SecretInjection>,
    pub allowed_egress_hosts: HashSet<String>,
    pub denied_hosts: HashSet<String>,
    pub strict_deny: bool,
}

/// How a placeholder is represented in a child request and rewritten by the broker.
#[derive(Debug, Clone)]
pub struct SecretInjection {
    pub header: String,
    pub value_template: String,
}

impl SecretInjection {
    pub fn bearer() -> Self {
        Self {
            header: "authorization".to_owned(),
            value_template: "Bearer {secret}".to_owned(),
        }
    }
}

impl BrokerPolicy {
    pub fn permissive() -> Self {
        Self {
            allowed_hosts_by_secret: HashMap::new(),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            strict_deny: false,
        }
    }
}

#[derive(Clone)]
struct BrokerState {
    secrets: Arc<HashMap<String, String>>,
    policy: BrokerPolicy,
    client: reqwest::Client,
    certificate_authority: Arc<Certificate>,
    audit_log: Option<AuditLog>,
    connections: Arc<ActiveConnections>,
}

/// Tracks every accepted proxy and TLS-upgrade task so broker shutdown closes
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
        let mut state = self.inner.lock().expect("broker connection lock poisoned");
        if state.stopped {
            task.abort();
        } else {
            state.tasks.push(task);
        }
    }

    fn stop(&self) {
        let mut state = self.inner.lock().expect("broker connection lock poisoned");
        state.stopped = true;
        for task in state.tasks.drain(..) {
            task.abort();
        }
    }
}

/// Owns the listener and the temporary trust anchor for exactly one child process.
pub struct Broker {
    child_env: HashMap<String, String>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    // Keeping this file alive makes the CA available to the child. Drop removes it.
    ca_file: PathBuf,
    ca_subject: String,
    audit_log: Option<AuditLog>,
    connections: Arc<ActiveConnections>,
}

impl Broker {
    #[cfg(test)]
    pub async fn start(
        secrets: HashMap<String, String>,
        policy: BrokerPolicy,
        audit_log: Option<AuditLog>,
    ) -> Result<Self> {
        Self::start_with_port(secrets, policy, audit_log, None).await
    }

    pub async fn start_with_port(
        secrets: HashMap<String, String>,
        mut policy: BrokerPolicy,
        audit_log: Option<AuditLog>,
        broker_port: Option<u16>,
    ) -> Result<Self> {
        if broker_port == Some(0) {
            anyhow::bail!("--broker-port must be between 1 and 65535");
        }
        let (certificate_authority, ca_file, ca_subject) = create_certificate_authority()?;
        let bind_address = format!("127.0.0.1:{}", broker_port.unwrap_or(0));
        let listener = TcpListener::bind(&bind_address)
            .await
            .with_context(|| format!("failed to bind credential broker to {bind_address}"))?;
        let address = listener.local_addr()?;
        let placeholders = secrets
            .into_iter()
            .map(|(name, value)| (placeholder_for(&name), value))
            .collect::<HashMap<_, _>>();
        policy.allowed_hosts_by_secret = policy
            .allowed_hosts_by_secret
            .into_iter()
            .map(|(name, hosts)| (placeholder_for(&name), normalize_hosts(hosts)))
            .collect();
        policy.secret_injections = normalize_injections(policy.secret_injections)?;
        policy.allowed_egress_hosts = normalize_hosts(policy.allowed_egress_hosts);
        policy.denied_hosts = normalize_hosts(policy.denied_hosts);
        let connections = Arc::new(ActiveConnections::default());
        let state = BrokerState {
            secrets: Arc::new(placeholders),
            policy,
            // Forwarding must never use proxy variables inherited by Stashbase itself.
            client: reqwest::Client::builder()
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
                // Return redirects to the child, which will make its next request through
                // this proxy and therefore re-run destination policy checks.
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            certificate_authority: Arc::new(certificate_authority),
            audit_log: audit_log.clone(),
            connections: connections.clone(),
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
            // Node's built-in fetch requires this opt-in before it reads proxy variables.
            ("NODE_USE_ENV_PROXY".to_owned(), "1".to_owned()),
            ("NO_PROXY".to_owned(), String::new()),
            ("no_proxy".to_owned(), String::new()),
        ]);
        for placeholder in state.secrets.keys() {
            child_env.insert(
                secret_name_from_placeholder(placeholder),
                placeholder.clone(),
            );
        }

        if let Some(audit_log) = &audit_log {
            audit_log.record("session_started", None, None, None, None, None);
        }

        Ok(Self {
            child_env,
            shutdown: Some(shutdown),
            task: Some(task),
            ca_file,
            ca_subject,
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
        super::trust::install(&self.ca_file, &self.ca_subject)
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.connections.stop();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _ = std::fs::remove_file(&self.ca_file);
    }
}

fn placeholder_for(name: &str) -> String {
    format!("**STASHBASE_{name}**")
}

fn secret_name_from_placeholder(placeholder: &str) -> String {
    placeholder
        .trim_start_matches("**STASHBASE_")
        .trim_end_matches("**")
        .to_owned()
}

fn create_certificate_authority() -> Result<(Certificate, PathBuf, String)> {
    let subject = format!("Stashbase Broker {}", Uuid::new_v4());
    let mut params = CertificateParams::new(vec!["stashbase-broker.local".to_owned()]);
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
    let path = std::env::temp_dir().join(format!("stashbase-broker-ca-{}.pem", Uuid::new_v4()));
    std::fs::write(&path, ca.serialize_pem()?).context("failed to write temporary broker CA")?;
    Ok((ca, path, subject))
}

async fn run_listener(
    listener: TcpListener,
    state: BrokerState,
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
    state: BrokerState,
    connect_authority: Option<String>,
) -> ProxyFuture {
    Box::pin(async move {
        let started = Instant::now();
        if request.method() == Method::CONNECT {
            let authority = request.uri().authority().map(|value| value.to_string());
            let Some(authority) = authority else {
                return Ok(broker_error_response(
                    StatusCode::BAD_REQUEST,
                    "broker.invalid_connect",
                    "CONNECT requires an authority",
                ));
            };
            if !state.host_allowed(Some(host_from_authority(&authority))) {
                debug!(
                    "broker denied destination: {}",
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
                return Ok(broker_error_response(
                    StatusCode::FORBIDDEN,
                    "broker.host_denied",
                    "Broker policy denied destination",
                ));
            }
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
                        let _ = serve_tls_connection(upgraded, authority, connection_state).await;
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
        if !state.host_allowed(host.as_deref()) {
            debug!(
                "broker denied destination: {}",
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
            return Ok(broker_error_response(
                StatusCode::FORBIDDEN,
                "broker.host_denied",
                "Broker policy denied destination",
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
            return Ok(broker_error_response(
                StatusCode::FORBIDDEN,
                "broker.unknown_placeholder",
                "Broker received an unknown credential placeholder",
            ));
        }
        let secret_name = match replace_placeholder(&mut request, &state, host.as_deref()) {
            Ok(secret_name) => secret_name,
            Err(secret_name) => {
                debug!(
                    "broker denied credential injection for destination: {}",
                    host.as_deref().unwrap_or("unknown")
                );
                state.record_audit(
                    "host_denied",
                    host.as_deref(),
                    Some(request.method()),
                    Some(&secret_name),
                    Some(StatusCode::FORBIDDEN),
                    Some(started.elapsed()),
                );
                return Ok(broker_error_response(
                    StatusCode::FORBIDDEN,
                    "broker.credential_host_denied",
                    "Broker policy denied credential",
                ));
            }
        };
        // Reqwest deliberately does not support HTTP upgrade responses. Coding agents
        // such as Codex use a WSS connection for streaming, so tunnel an upgraded
        // connection after applying the same destination and placeholder checks.
        if is_upgrade_request(&request) {
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
                return Ok(broker_error_response(
                    StatusCode::BAD_REQUEST,
                    "broker.request_invalid",
                    "Unable to determine request URL",
                ));
            }
        };
        let method = request.method().clone();
        let headers = request.headers().clone();
        // `Incoming` is converted into a data stream without collecting it. Reqwest
        // applies chunked transfer encoding when no content length is available, so
        // streaming uploads retain their incremental delivery to the upstream.
        let body = reqwest::Body::wrap_stream(request.into_body().into_data_stream());

        match state
            .client
            .request(method.clone(), url)
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
                    "broker could not forward request to destination: {}",
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
                Ok(broker_error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("broker.{}", upstream_error_action(&error)),
                    "Unable to forward broker request",
                ))
            }
        }
    })
}

fn is_upgrade_request(request: &Request<Incoming>) -> bool {
    request.headers().contains_key(hyper::header::UPGRADE)
}

/// For WebSockets and other HTTP/1 upgrades, make the upstream connection with
/// Hyper rather than Reqwest, then copy the two upgraded byte streams. The
/// request has already passed policy checks and placeholder replacement.
async fn forward_upgrade(
    request: Request<Incoming>,
    state: BrokerState,
    connect_authority: Option<String>,
    host: Option<String>,
    secret_name: Option<String>,
    started: Instant,
) -> Result<Response<ProxyBody>, Infallible> {
    let authority = match upstream_authority(&request, connect_authority.as_deref()) {
        Ok(authority) => authority,
        Err(_) => {
            return Ok(broker_error_response(
                StatusCode::BAD_REQUEST,
                "broker.request_invalid",
                "Unable to determine request URL",
            ));
        }
    };
    let (hostname, port) = match split_authority(&authority, connect_authority.is_some()) {
        Some(parts) => parts,
        None => {
            return Ok(broker_error_response(
                StatusCode::BAD_REQUEST,
                "broker.request_invalid",
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
                return Ok(broker_error_response(
                    StatusCode::BAD_REQUEST,
                    "broker.request_invalid",
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
    state: BrokerState,
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
    state: &BrokerState,
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
    broker_error_response(
        StatusCode::BAD_GATEWAY,
        "broker.upgrade_failed",
        "Unable to establish upgraded broker connection",
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
    state: BrokerState,
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

impl BrokerState {
    fn host_allowed(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return !self.policy.strict_deny;
        };
        !policy_denies_host(&self.policy, host)
            && (!self.policy.strict_deny || policy_allows_host(&self.policy, host))
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

fn policy_allows_host(policy: &BrokerPolicy, host: &str) -> bool {
    policy
        .allowed_hosts_by_secret
        .values()
        .any(|hosts| hosts.iter().any(|allowed| host_matches(allowed, host)))
        || policy
            .allowed_egress_hosts
            .iter()
            .any(|allowed| allowed == "*" || host_matches(allowed, host))
}

fn policy_denies_host(policy: &BrokerPolicy, host: &str) -> bool {
    policy
        .denied_hosts
        .iter()
        .any(|denied| denied == "*" || host_matches(denied, host))
}

fn replace_placeholder(
    request: &mut Request<Incoming>,
    state: &BrokerState,
    host: Option<&str>,
) -> std::result::Result<Option<String>, String> {
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
            && !state
                .policy
                .allowed_hosts_by_secret
                .get(placeholder)
                .is_some_and(|hosts| {
                    host.is_some_and(|host| hosts.iter().any(|allowed| host_matches(allowed, host)))
                })
        {
            return Err(secret_name_from_placeholder(placeholder));
        }
        let value = injection.value_template.replace("{secret}", secret);
        if let Ok(value) = HeaderValue::from_str(&value) {
            request.headers_mut().insert(header_name, value);
        }
        return Ok(Some(secret_name_from_placeholder(placeholder)));
    }

    Ok(None)
}

/// Reject placeholder-shaped values that do not belong to this session instead
/// of forwarding them to an upstream service. This avoids accidental leakage of
/// a placeholder and makes stale profile bindings diagnosable from audit logs.
fn contains_unknown_placeholder(request: &Request<Incoming>, state: &BrokerState) -> bool {
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
            Ok((
                placeholder_for(&name),
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

/// Broker failures use the public API error envelope so a nested `stashbase`
/// command can report policy denials clearly instead of failing JSON parsing.
fn broker_error_response(status: StatusCode, code: &str, message: &str) -> Response<ProxyBody> {
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
    use hyper::header::{AUTHORIZATION, TRANSFER_ENCODING};
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

    fn proxy_client(broker: &Broker) -> reqwest::Client {
        reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(&broker.child_env()["HTTP_PROXY"]).unwrap())
            .build()
            .unwrap()
    }

    #[test]
    fn creates_expected_placeholders() {
        assert_eq!(placeholder_for("GH_TOKEN"), "**STASHBASE_GH_TOKEN**");
    }

    #[tokio::test]
    async fn broker_errors_use_the_api_error_envelope() {
        let response = broker_error_response(
            StatusCode::FORBIDDEN,
            "broker.host_denied",
            "Broker policy denied destination",
        );
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let error: crate::models::api_client::ApiErrorResponse =
            serde_json::from_slice(&body).unwrap();
        assert_eq!(error.error.code, "broker.host_denied");
        assert_eq!(
            error.error.message.as_deref(),
            Some("Broker policy denied destination")
        );
    }

    #[tokio::test]
    async fn broker_stop_closes_the_listener() {
        let broker = Broker::start(HashMap::new(), BrokerPolicy::permissive(), None)
            .await
            .unwrap();
        let address: std::net::SocketAddr = broker.child_env()["HTTP_PROXY"]
            .trim_start_matches("http://")
            .parse()
            .unwrap();

        assert!(tokio::net::TcpStream::connect(address).await.is_ok());
        broker.stop().await;
        let mut closed = false;
        for _ in 0..10 {
            if tokio::net::TcpStream::connect(address).await.is_err() {
                closed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(closed, "broker listener remained reachable after stop");
    }

    #[tokio::test]
    async fn broker_uses_an_explicit_local_port() {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);

        let broker =
            Broker::start_with_port(HashMap::new(), BrokerPolicy::permissive(), None, Some(port))
                .await
                .unwrap();

        assert_eq!(
            broker.child_env()["HTTP_PROXY"],
            format!("http://127.0.0.1:{port}")
        );
        broker.stop().await;
    }

    #[tokio::test]
    async fn broker_rejects_port_zero_override() {
        let result =
            Broker::start_with_port(HashMap::new(), BrokerPolicy::permissive(), None, Some(0))
                .await;
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
        let audit_log = AuditLog {
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

        prune_audit_logs(&directory).unwrap();

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
        let event = AuditLogEvent {
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

        assert!(AuditLogFilter {
            profile: Some("coding".to_owned()),
            action: Some("injected".to_owned()),
            host: Some("api.github.com".to_owned()),
            session: Some("session-1".to_owned()),
        }
        .matches(&event));
        assert!(!AuditLogFilter {
            host: Some("example.com".to_owned()),
            ..Default::default()
        }
        .matches(&event));
    }

    #[test]
    fn strict_policy_allows_only_configured_hosts() {
        let policy = BrokerPolicy {
            allowed_hosts_by_secret: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                normalize_hosts(HashSet::from(["API.GITHUB.COM.".to_owned()])),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::new(),
            denied_hosts: HashSet::new(),
            strict_deny: true,
        };

        assert!(policy_allows_host(&policy, "api.github.com"));
        assert!(!policy_allows_host(&policy, "example.com"));
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

    #[test]
    fn egress_wildcard_allows_any_destination_without_widening_secret_hosts() {
        let policy = BrokerPolicy {
            allowed_hosts_by_secret: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                HashSet::from(["api.github.com".to_owned()]),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::new(),
            strict_deny: true,
        };

        assert!(policy_allows_host(&policy, "example.com"));
        assert!(!policy.allowed_hosts_by_secret["**STASHBASE_GH_TOKEN**"]
            .iter()
            .any(|allowed| host_matches(allowed, "example.com")));
    }

    #[test]
    fn denied_hosts_override_wildcard_egress_and_secret_destinations() {
        let policy = BrokerPolicy {
            allowed_hosts_by_secret: HashMap::from([(
                "**STASHBASE_GH_TOKEN**".to_owned(),
                HashSet::from(["api.stashbase.dev".to_owned()]),
            )]),
            secret_injections: HashMap::new(),
            allowed_egress_hosts: HashSet::from(["*".to_owned()]),
            denied_hosts: HashSet::from(["api.stashbase.dev".to_owned()]),
            strict_deny: true,
        };
        let state = BrokerState {
            secrets: Arc::new(HashMap::new()),
            policy,
            client: reqwest::Client::new(),
            certificate_authority: Arc::new(
                Certificate::from_params(CertificateParams::default()).unwrap(),
            ),
            audit_log: None,
            connections: Arc::new(ActiveConnections::default()),
        };

        assert!(state.host_allowed(Some("chatgpt.com")));
        assert!(!state.host_allowed(Some("api.stashbase.dev")));
    }

    #[tokio::test]
    async fn rewrites_a_placeholder_before_forwarding_a_http_request() {
        let (address, authorization) = start_backend().await;
        let broker = Broker::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            BrokerPolicy::permissive(),
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&broker)
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
        broker.stop().await;
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

        let broker = Broker::start(HashMap::new(), BrokerPolicy::permissive(), None)
            .await
            .unwrap();
        let proxy = broker.child_env()["HTTP_PROXY"].trim_start_matches("http://");
        let stream = TcpStream::connect(proxy).await.unwrap();
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
        broker.stop().await;
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
        let broker = Broker::start(HashMap::new(), BrokerPolicy::permissive(), None)
            .await
            .unwrap();

        let payload = r#"{"model":"example","stream":true}"#;
        let response = proxy_client(&broker)
            .post(format!("http://{address}/v1/chat"))
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body.await.unwrap(), Bytes::from(payload));
        broker.stop().await;
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
        let broker = Broker::start(HashMap::new(), BrokerPolicy::permissive(), None)
            .await
            .unwrap();
        let client = proxy_client(&broker);
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
            .expect("the first chunk was buffered by the broker")
            .unwrap();
        assert_eq!(transfer_encoding.as_deref(), Some("chunked"));
        assert_eq!(first_chunk, Bytes::from_static(b"first"));
        assert_eq!(request.await.unwrap().status(), StatusCode::OK);
        broker.stop().await;
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
        let broker = Broker::start(HashMap::new(), BrokerPolicy::permissive(), None)
            .await
            .unwrap();

        let response = proxy_client(&broker)
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
                .expect("the first SSE event was buffered by the broker")
                .unwrap()
                .unwrap(),
            Bytes::from_static(b"data: first\n\n")
        );
        assert_eq!(
            events.next().await.unwrap().unwrap(),
            Bytes::from_static(b"data: second\n\n")
        );
        broker.stop().await;
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
        let broker = Broker::start(HashMap::new(), BrokerPolicy::permissive(), None)
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

        let mut response = proxy_client(&broker)
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
        broker.stop().await;
    }

    #[tokio::test]
    async fn rejects_and_audits_an_unknown_placeholder_without_recording_it() {
        let path =
            std::env::temp_dir().join(format!("stashbase-audit-test-{}.jsonl", Uuid::new_v4()));
        let audit_log = AuditLog {
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
        let broker = Broker::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            BrokerPolicy::permissive(),
            Some(audit_log),
        )
        .await
        .unwrap();

        let response = proxy_client(&broker)
            .get("http://127.0.0.1:1/")
            .header(AUTHORIZATION, "Bearer **STASHBASE_STALE_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        broker.stop().await;

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("unknown_placeholder"));
        assert!(!content.contains("STASHBASE_STALE_TOKEN"));
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn rewrites_a_placeholder_in_a_configured_api_key_header() {
        let header_name = HeaderName::from_static("x-api-key");
        let (address, api_key) = start_backend_capturing(header_name.clone()).await;
        let broker = Broker::start(
            HashMap::from([("ANTHROPIC_API_KEY".to_owned(), "real-token".to_owned())]),
            BrokerPolicy {
                allowed_hosts_by_secret: HashMap::from([(
                    "ANTHROPIC_API_KEY".to_owned(),
                    HashSet::from(["127.0.0.1".to_owned()]),
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
                strict_deny: true,
            },
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&broker)
            .get(format!("http://{address}/"))
            .header("x-api-key", "**STASHBASE_ANTHROPIC_API_KEY**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(api_key.await.unwrap().as_deref(), Some("real-token"));
        broker.stop().await;
    }

    #[tokio::test]
    async fn strict_policy_denies_unapproved_destinations_and_sets_node_environment() {
        let (address, _authorization) = start_backend().await;
        let broker = Broker::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            BrokerPolicy {
                allowed_hosts_by_secret: HashMap::from([(
                    "GH_TOKEN".to_owned(),
                    HashSet::from(["api.github.com".to_owned()]),
                )]),
                secret_injections: HashMap::new(),
                allowed_egress_hosts: HashSet::new(),
                denied_hosts: HashSet::new(),
                strict_deny: true,
            },
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&broker)
            .get(format!("http://{address}/"))
            .header(AUTHORIZATION, "Bearer **STASHBASE_GH_TOKEN**")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(broker.child_env()["NODE_USE_ENV_PROXY"], "1");
        assert!(std::path::Path::new(&broker.child_env()["NODE_EXTRA_CA_CERTS"]).exists());
        broker.stop().await;
    }

    #[tokio::test]
    async fn egress_only_host_is_forwarded_without_credential_injection() {
        let (address, authorization) = start_backend().await;
        let broker = Broker::start(
            HashMap::from([("GH_TOKEN".to_owned(), "real-token".to_owned())]),
            BrokerPolicy {
                allowed_hosts_by_secret: HashMap::from([(
                    "GH_TOKEN".to_owned(),
                    HashSet::from(["api.github.com".to_owned()]),
                )]),
                secret_injections: HashMap::new(),
                allowed_egress_hosts: HashSet::from(["127.0.0.1".to_owned()]),
                denied_hosts: HashSet::new(),
                strict_deny: true,
            },
            None,
        )
        .await
        .unwrap();

        let response = proxy_client(&broker)
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(authorization.await.unwrap(), None);
        broker.stop().await;
    }
}
