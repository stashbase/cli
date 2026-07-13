//! A deliberately small, per-command HTTP proxy used by `stashbase run --broker`.
//!
//! HTTPS traffic is intercepted with a temporary locally-trusted CA so the proxy can
//! replace Stashbase placeholders in request headers before forwarding the request.
//! This is an experiment, not a hardened proxy implementation.

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
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
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

use crate::REQUEST_TIMEOUT_SECS;

type ProxyBody = Full<Bytes>;
type ProxyFuture = Pin<Box<dyn Future<Output = Result<Response<ProxyBody>, Infallible>> + Send>>;

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
}

/// Owns the listener and the temporary trust anchor for exactly one child process.
pub struct Broker {
    child_env: HashMap<String, String>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    // Keeping this file alive makes the CA available to the child. Drop removes it.
    ca_file: PathBuf,
    ca_subject: String,
}

impl Broker {
    pub async fn start(secrets: HashMap<String, String>, mut policy: BrokerPolicy) -> Result<Self> {
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

        Ok(Self {
            child_env,
            shutdown: Some(shutdown),
            task: Some(task),
            ca_file,
            ca_subject,
        })
    }

    pub fn child_env(&self) -> &HashMap<String, String> {
        &self.child_env
    }

    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    pub fn trust_ca(&self) -> Result<super::trust::TemporaryCaTrust> {
        super::trust::install(&self.ca_file, &self.ca_subject)
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
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
                    tokio::spawn(async move {
                        let service = service_fn(move |request| proxy_request(request, state.clone(), None));
                        let _ = http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .with_upgrades()
                            .await;
                    });
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
                return Ok(response(
                    StatusCode::FORBIDDEN,
                    "Broker policy denied destination",
                ));
            }
            tokio::spawn(async move {
                if let Ok(upgraded) = hyper::upgrade::on(&mut request).await {
                    let _ = serve_tls_connection(upgraded, authority, state).await;
                }
            });
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
            return Ok(response(
                StatusCode::FORBIDDEN,
                "Broker policy denied destination",
            ));
        }
        if !replace_placeholder(&mut request, &state, host.as_deref()) {
            debug!(
                "broker denied credential injection for destination: {}",
                host.as_deref().unwrap_or("unknown")
            );
            return Ok(response(
                StatusCode::FORBIDDEN,
                "Broker policy denied credential",
            ));
        }
        let url = match request_url(&request, connect_authority.as_deref()) {
            Ok(url) => url,
            Err(_) => {
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "Unable to determine request URL",
                ))
            }
        };
        let method = request.method().clone();
        let headers = request.headers().clone();
        let body = match request.into_body().collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => {
                return Ok(response(
                    StatusCode::BAD_REQUEST,
                    "Unable to read request body",
                ))
            }
        };

        match state
            .client
            .request(method, url)
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
                        Ok(response)
                    }
                    Err(_) => Ok(response(
                        StatusCode::BAD_GATEWAY,
                        "Unable to read upstream response",
                    )),
                }
            }
            Err(_) => {
                debug!(
                    "broker could not forward request to destination: {}",
                    host.as_deref().unwrap_or("unknown")
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
) -> bool {
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
            return false;
        }
        let value = injection.value_template.replace("{secret}", secret);
        if let Ok(value) = HeaderValue::from_str(&value) {
            request.headers_mut().insert(header_name, value);
        }
        return true;
    }

    true
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
