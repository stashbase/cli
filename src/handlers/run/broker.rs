//! A deliberately small, per-command HTTP proxy used by `stashbase run --broker`.
//!
//! HTTPS traffic is intercepted with a temporary locally-trusted CA so the proxy can
//! replace Stashbase placeholders in request headers before forwarding the request.
//! This is an experiment, not a hardened proxy implementation.

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
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    header::{HeaderName, HeaderValue},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use log::debug;
use rcgen::{BasicConstraints, Certificate, CertificateParams, DnType, IsCa, KeyUsagePurpose};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConfig,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

use crate::REQUEST_TIMEOUT_SECS;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

type ProxyBody = Full<Bytes>;
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
    pub async fn start(
        secrets: HashMap<String, String>,
        mut policy: BrokerPolicy,
        audit_log: Option<AuditLog>,
    ) -> Result<Self> {
        let (certificate_authority, ca_file, ca_subject) = create_certificate_authority()?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind credential broker to localhost")?;
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
        let connections = Arc::new(ActiveConnections::default());
        let state = BrokerState {
            secrets: Arc::new(placeholders),
            policy,
            // Forwarding must never use proxy variables inherited by Stashbase itself.
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(
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
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "CONNECT requires an authority",
                ));
            };
            if !state.host_allowed(Some(host_from_authority(&authority))) {
                debug!(
                    "broker denied destination: {}",
                    host_from_authority(&authority)
                );
                state.record_audit(
                    "denied_destination",
                    Some(host_from_authority(&authority)),
                    Some(&Method::CONNECT),
                    None,
                    Some(StatusCode::FORBIDDEN),
                    Some(started.elapsed()),
                );
                return Ok(response(
                    StatusCode::FORBIDDEN,
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
            let task = tokio::spawn(async move {
                if let Ok(upgraded) = hyper::upgrade::on(&mut request).await {
                    let _ = serve_tls_connection(upgraded, authority, state).await;
                }
            });
            connections.track(task);
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::new()))
                .unwrap());
        }

        let host = request_host(&request, connect_authority.as_deref());
        if !state.host_allowed(host.as_deref()) {
            debug!(
                "broker denied destination: {}",
                host.as_deref().unwrap_or("unknown")
            );
            state.record_audit(
                "denied_destination",
                host.as_deref(),
                Some(request.method()),
                None,
                Some(StatusCode::FORBIDDEN),
                Some(started.elapsed()),
            );
            return Ok(response(
                StatusCode::FORBIDDEN,
                "Broker policy denied destination",
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
                    "denied_credential",
                    host.as_deref(),
                    Some(request.method()),
                    Some(&secret_name),
                    Some(StatusCode::FORBIDDEN),
                    Some(started.elapsed()),
                );
                return Ok(response(
                    StatusCode::FORBIDDEN,
                    "Broker policy denied credential",
                ));
            }
        };
        let url = match request_url(&request, connect_authority.as_deref()) {
            Ok(url) => url,
            Err(_) => {
                state.record_audit(
                    "invalid_request",
                    host.as_deref(),
                    Some(request.method()),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_REQUEST),
                    Some(started.elapsed()),
                );
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "Unable to determine request URL",
                ));
            }
        };
        let method = request.method().clone();
        let headers = request.headers().clone();
        let body = match request.into_body().collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => {
                state.record_audit(
                    "invalid_request_body",
                    host.as_deref(),
                    Some(&method),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_REQUEST),
                    Some(started.elapsed()),
                );
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "Unable to read request body",
                ));
            }
        };

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
                match upstream.bytes().await {
                    Ok(body) => {
                        let mut response = Response::builder()
                            .status(status)
                            .body(Full::new(body))
                            .unwrap();
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
                    Err(_) => {
                        state.record_audit(
                            "upstream_failed",
                            host.as_deref(),
                            Some(&method),
                            secret_name.as_deref(),
                            Some(StatusCode::BAD_GATEWAY),
                            Some(started.elapsed()),
                        );
                        Ok(response(
                            StatusCode::BAD_GATEWAY,
                            "Unable to read upstream response",
                        ))
                    }
                }
            }
            Err(_) => {
                debug!(
                    "broker could not forward request to destination: {}",
                    host.as_deref().unwrap_or("unknown")
                );
                state.record_audit(
                    "upstream_failed",
                    host.as_deref(),
                    Some(&method),
                    secret_name.as_deref(),
                    Some(StatusCode::BAD_GATEWAY),
                    Some(started.elapsed()),
                );
                Ok(response(
                    StatusCode::BAD_GATEWAY,
                    "Unable to forward broker request",
                ))
            }
        }
    })
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
    let stream = TlsAcceptor::from(Arc::new(config))
        .accept(TokioIo::new(upgraded))
        .await?;
    let service =
        service_fn(move |request| proxy_request(request, state.clone(), Some(authority.clone())));
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await?;
    Ok(())
}

impl BrokerState {
    fn host_allowed(&self, host: Option<&str>) -> bool {
        !self.policy.strict_deny || host.is_some_and(|host| policy_allows_host(&self.policy, host))
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

fn response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::from(message.to_owned())))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::AUTHORIZATION;
    use tokio::sync::oneshot;

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
            strict_deny: true,
        };

        assert!(policy_allows_host(&policy, "example.com"));
        assert!(!policy.allowed_hosts_by_secret["**STASHBASE_GH_TOKEN**"]
            .iter()
            .any(|allowed| host_matches(allowed, "example.com")));
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
