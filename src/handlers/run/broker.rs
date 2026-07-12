//! A deliberately small, per-command HTTP proxy used by `stashbase run --broker`.
//!
//! HTTPS traffic is intercepted with a temporary locally-trusted CA so the proxy can
//! replace Stashbase placeholders in request headers before forwarding the request.
//! This is an experiment, not a hardened proxy implementation.

use std::{
    collections::HashMap, convert::Infallible, future::Future, path::PathBuf, pin::Pin, sync::Arc,
};

use anyhow::{Context, Result};
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    header::{HeaderValue, AUTHORIZATION},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use rcgen::{BasicConstraints, Certificate, CertificateParams, IsCa, KeyUsagePurpose};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConfig,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

type ProxyBody = Full<Bytes>;
type ProxyFuture = Pin<Box<dyn Future<Output = Result<Response<ProxyBody>, Infallible>> + Send>>;

#[derive(Clone)]
struct BrokerState {
    secrets: Arc<HashMap<String, String>>,
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
}

impl Broker {
    pub async fn start(secrets: HashMap<String, String>) -> Result<Self> {
        let (certificate_authority, ca_file) = create_certificate_authority()?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind credential broker to localhost")?;
        let address = listener.local_addr()?;
        let placeholders = secrets
            .into_iter()
            .map(|(name, value)| (placeholder_for(&name), value))
            .collect::<HashMap<_, _>>();
        let state = BrokerState {
            secrets: Arc::new(placeholders),
            // Forwarding must never use proxy variables inherited by Stashbase itself.
            client: reqwest::Client::builder().no_proxy().build()?,
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
            ("GIT_SSL_CAINFO".to_owned(), ca_path),
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

fn create_certificate_authority() -> Result<(Certificate, PathBuf)> {
    let mut params = CertificateParams::new(vec!["stashbase-broker.local".to_owned()]);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let ca = Certificate::from_params(params)?;
    let path = std::env::temp_dir().join(format!("stashbase-broker-ca-{}.pem", Uuid::new_v4()));
    std::fs::write(&path, ca.serialize_pem()?).context("failed to write temporary broker CA")?;
    Ok((ca, path))
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

        replace_placeholder(&mut request, &state.secrets);
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
            Err(_) => Ok(response(
                StatusCode::BAD_GATEWAY,
                "Unable to forward broker request",
            )),
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

fn replace_placeholder(request: &mut Request<Incoming>, secrets: &HashMap<String, String>) {
    let Some(header) = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return;
    };
    let Some(placeholder) = header.strip_prefix("Bearer ") else {
        return;
    };
    let Some(secret) = secrets.get(placeholder) else {
        return;
    };
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {secret}")) {
        request.headers_mut().insert(AUTHORIZATION, value);
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

    #[test]
    fn creates_expected_placeholders() {
        assert_eq!(placeholder_for("GH_TOKEN"), "**STASHBASE_GH_TOKEN**");
    }
}
