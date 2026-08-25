use std::{sync::Mutex, time::Duration};

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::{
    api::{auth::get_current_auth_details, client},
    models::{
        agent::AgentHttpRule,
        api_client::{ApiErrorResponse, GenericOutputError, GetRequestApiResponse, OutputError},
        auth::CurrentAuthResponse,
    },
};

#[derive(Clone)]
struct ActiveAgentRunCleanup {
    api_key: String,
    session_token: String,
}

// One CLI process runs one foreground agent command at a time. This registry
// lets Unix termination handlers complete best-effort remote cleanup without
// putting an API key or session token in the child environment.
static ACTIVE_AGENT_RUN_CLEANUP: Lazy<Mutex<Option<ActiveAgentRunCleanup>>> =
    Lazy::new(|| Mutex::new(None));

// Cleanup is best-effort because server-side expiry remains the safe fallback.
// Do not make an interactive agent wait for Reqwest's default timeout when the
// control plane is unavailable during shutdown or session rotation.
const CLEANUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub fn register_agent_run_cleanup(api_key: String, session_token: String) {
    if let Ok(mut active) = ACTIVE_AGENT_RUN_CLEANUP.lock() {
        *active = Some(ActiveAgentRunCleanup {
            api_key,
            session_token,
        });
    }
}

pub fn update_agent_run_cleanup_token(session_token: String) {
    if let Ok(mut active) = ACTIVE_AGENT_RUN_CLEANUP.lock() {
        if let Some(active) = active.as_mut() {
            active.session_token = session_token;
        }
    }
}

pub fn clear_agent_run_cleanup() {
    if let Ok(mut active) = ACTIVE_AGENT_RUN_CLEANUP.lock() {
        *active = None;
    }
}

/// Ends and clears the registered run, if one exists. Used only for graceful
/// process termination; SIGKILL cannot run this cleanup.
pub async fn end_registered_agent_run() {
    let active = ACTIVE_AGENT_RUN_CLEANUP
        .lock()
        .ok()
        .and_then(|mut active| active.take());
    if let Some(active) = active {
        end_agent_run(active.api_key, &active.session_token).await;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteBinding {
    pub name: String,
    /// The control plane resolves project/environment secrets or personal
    /// credentials based on this source; the CLI never receives their values.
    pub source: RemoteBindingSource,
    /// Canonical name in the selected source. This is metadata only; the CLI
    /// never receives the resolved value.
    pub source_name: String,
    pub hosts: Vec<String>,
    pub rules: Vec<AgentHttpRule>,
    pub header: String,
    pub placeholder: String,
    pub value_template: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteBindingSource {
    Secret,
    PersonalCredential,
}

/// Everything needed to issue a replacement session. This remains in memory for
/// the duration of one agent command only.
#[derive(Clone)]
pub struct RemoteProxySessionRequest {
    pub api_key: String,
    /// Required when the session contains project/environment secret bindings.
    pub project_identifier: Option<String>,
    pub environment_identifier: Option<String>,
    /// Ordinary destinations the agent may reach without secret injection.
    /// Per-secret credential policy remains in each binding's legacy `hosts`
    /// field or its HTTP `rules`.
    pub egress_hosts: Vec<String>,
    pub deny_hosts: Vec<String>,
    pub bindings: Vec<RemoteBinding>,
    /// Sent only for the initial logical agent session. The control plane keeps
    /// that value when a replacement session is issued.
    pub agent_type: Option<String>,
    /// Present only while rotating an existing logical agent session.
    pub previous_session_token: Option<String>,
}

impl RemoteProxySessionRequest {
    pub fn replacement(&self, previous_session_token: String) -> Self {
        let mut request = self.clone();
        request.agent_type = None;
        request.previous_session_token = Some(previous_session_token);
        request
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoteProxySession {
    pub session_id: String,
    pub session_token: String,
    pub expires_at: String,
    pub proxy_url: String,
    pub protocol: String,
    pub proxy_ca: Option<RemoteProxyCa>,
}

/// Public trust material for the remote TLS-intercepting forward proxy. This
/// never contains a private key or any credential.
#[derive(Debug, Deserialize)]
pub struct RemoteProxyCa {
    pub key_id: String,
    pub sha256: String,
    pub pem: String,
}

#[derive(Serialize)]
struct CreateSession<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment_id: Option<&'a str>,
    egress_hosts: &'a [String],
    deny_hosts: &'a [String],
    bindings: &'a [RemoteBinding],
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_type: Option<&'a str>,
}

pub async fn create_session(
    request: &RemoteProxySessionRequest,
    json_format: bool,
) -> Result<RemoteProxySession> {
    if request
        .bindings
        .iter()
        .any(|binding| binding.source == RemoteBindingSource::PersonalCredential)
    {
        ensure_user_authentication_for_credentials(&request.api_key, json_format).await?;
    }
    let client = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()?;
    let response = create_session_http_request(&client, request)
        .send()
        .await
        .context("failed to create remote Agent Proxy session")?;
    if !response.status().is_success() {
        let status = response.status();
        let error_response = response.json::<ApiErrorResponse>().await.ok();
        bail!(format_session_error(
            status,
            error_response,
            json_format,
            request
                .bindings
                .iter()
                .any(|binding| binding.source == RemoteBindingSource::PersonalCredential),
        )?);
    }
    let mut session: RemoteProxySession = response
        .json()
        .await
        .context("invalid remote Agent Proxy session response")?;
    if session.proxy_url.starts_with('/') {
        session.proxy_url = format!("{}{}", client::get_api_url(), session.proxy_url);
    }
    Ok(session)
}

async fn ensure_user_authentication_for_credentials(
    api_key: &str,
    json_format: bool,
) -> Result<()> {
    let response = match get_current_auth_details(api_key.to_owned()).await {
        Ok(response) => response,
        Err(error) => bail!(error.format_error_output(json_format)?),
    };
    let GetRequestApiResponse::Ok(response) = response else {
        let GetRequestApiResponse::Err(error) = response else {
            unreachable!()
        };
        bail!(error.format_error_output(json_format)?);
    };
    let auth: CurrentAuthResponse = serde_json::from_str(&response.text)
        .context("invalid authentication response while checking credential access")?;
    if matches!(auth, CurrentAuthResponse::ServiceAccount { .. }) {
        bail!("Service API keys cannot create Remote Agent sessions with personal credential bindings.");
    }
    Ok(())
}

/// Formats HTTP responses from the Agent Proxy control plane with the same
/// stable envelope used by the rest of the CLI. Session tokens and bindings are
/// never included in this error path.
fn format_session_error(
    status: reqwest::StatusCode,
    response: Option<ApiErrorResponse>,
    json_format: bool,
    requested_credentials: bool,
) -> Result<String> {
    if requested_credentials && response.as_ref().is_some_and(credential_access_is_disabled) {
        return Ok("Enable Personal credential access in Workspace → Agents before starting this Remote Agent session.".to_owned());
    }
    let error = match response {
        Some(response) => OutputError::from(response.error),
        None => OutputError::Generic(GenericOutputError {
            code: None,
            message: "Agent Proxy session request failed.".to_owned(),
            status: None,
            hint: None,
            details: None,
        }),
    }
    .with_status(Some(status.as_u16()));
    Ok(error.format_error_output(json_format)?)
}

fn credential_access_is_disabled(response: &ApiErrorResponse) -> bool {
    let code = response.error.code.to_ascii_lowercase();
    let message = response
        .error
        .message
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    (code.contains("credential") && (code.contains("disabled") || code.contains("not_enabled")))
        || (message.contains("credential")
            && (message.contains("disabled") || message.contains("not enabled")))
}

fn create_session_http_request(
    client: &reqwest::Client,
    request: &RemoteProxySessionRequest,
) -> reqwest::RequestBuilder {
    let mut session_request = client
        .post(format!("{}/v1/agent-proxy/sessions", client::get_api_url()))
        .bearer_auth(&request.api_key)
        .json(&CreateSession {
            project_id: request.project_identifier.as_deref(),
            environment_id: request.environment_identifier.as_deref(),
            egress_hosts: &request.egress_hosts,
            deny_hosts: &request.deny_hosts,
            bindings: &request.bindings,
            agent_type: request.agent_type.as_deref(),
        });
    if let Some(previous_session_token) = &request.previous_session_token {
        session_request =
            session_request.header("X-Stashbase-Previous-Session", previous_session_token);
    }
    session_request
}

pub async fn revoke_session(api_key: String, session_token: &str) {
    delete_session(api_key, session_token, false).await;
}

/// Revokes the active token and tells the control plane that this logical agent
/// invocation has ended. This is best-effort cleanup; abrupt process death is
/// represented by the server-side session expiry instead.
pub async fn end_agent_run(api_key: String, session_token: &str) {
    delete_session(api_key, session_token, true).await;
}

async fn delete_session(api_key: String, session_token: &str, end_agent_run: bool) {
    let Ok(client) = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .timeout(CLEANUP_REQUEST_TIMEOUT)
        .build()
    else {
        return;
    };
    let _ = delete_session_http_request(&client, api_key, session_token, end_agent_run)
        .send()
        .await;
}

fn delete_session_http_request(
    client: &reqwest::Client,
    api_key: String,
    session_token: &str,
    end_agent_run: bool,
) -> reqwest::RequestBuilder {
    let mut request = client
        .delete(format!(
            "{}/v1/agent-proxy/sessions/current",
            client::get_api_url()
        ))
        .bearer_auth(api_key)
        .header("X-Stashbase-Session", session_token);
    if end_agent_run {
        request = request.header("X-Stashbase-End-Agent-Run", "true");
    }
    request
}

/// Marks a replaced session for the server-side grace period. The raw token is
/// supplied only as a request header and is never persisted by the CLI.
pub async fn retire_session(api_key: String, session_token: &str) {
    let Ok(client) = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .timeout(CLEANUP_REQUEST_TIMEOUT)
        .build()
    else {
        return;
    };
    let _ = client
        .post(format!(
            "{}/v1/agent-proxy/sessions/current/retire",
            client::get_api_url()
        ))
        .bearer_auth(api_key)
        .header("X-Stashbase-Session", session_token)
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_request(previous_session_token: Option<&str>) -> RemoteProxySessionRequest {
        RemoteProxySessionRequest {
            api_key: "test-api-key".to_owned(),
            project_identifier: Some("project".to_owned()),
            environment_identifier: Some("environment".to_owned()),
            egress_hosts: vec!["api.example.com".to_owned()],
            deny_hosts: Vec::new(),
            bindings: Vec::new(),
            agent_type: Some("custom".to_owned()),
            previous_session_token: previous_session_token.map(str::to_owned),
        }
    }

    #[test]
    fn replacement_session_request_carries_the_previous_session_token() {
        let client = reqwest::Client::new();
        let request = create_session_http_request(&client, &session_request(Some("old-token")))
            .build()
            .unwrap();

        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().path(), "/v1/agent-proxy/sessions");
        assert_eq!(
            request
                .headers()
                .get("X-Stashbase-Previous-Session")
                .unwrap(),
            "old-token"
        );
        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .unwrap(),
            "Bearer test-api-key"
        );
    }

    #[test]
    fn session_request_keeps_egress_and_secret_destinations_separate() {
        let client = reqwest::Client::new();
        let mut session = session_request(None);
        session.egress_hosts = vec!["*".to_owned()];
        session.bindings = vec![RemoteBinding {
            name: "GH_TOKEN".to_owned(),
            source: RemoteBindingSource::Secret,
            source_name: "GH_TOKEN".to_owned(),
            hosts: vec!["api.github.com".to_owned(), "github.com".to_owned()],
            rules: Vec::new(),
            header: "authorization".to_owned(),
            placeholder: "${STASHBASE_GH_TOKEN}".to_owned(),
            value_template: "Bearer {value}".to_owned(),
        }];
        let request = create_session_http_request(&client, &session)
            .build()
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(request.body().and_then(reqwest::Body::as_bytes).unwrap())
                .unwrap();

        assert_eq!(body["egress_hosts"], serde_json::json!(["*"]));
        assert_eq!(
            body["bindings"][0]["hosts"],
            serde_json::json!(["api.github.com", "github.com"])
        );
        assert!(body.get("allowed_hosts").is_none());
        assert_eq!(body["bindings"][0]["source"], "secret");
        assert_eq!(body["bindings"][0]["source_name"], "GH_TOKEN");
        assert!(body["bindings"][0].get("secret_name").is_none());
    }

    #[test]
    fn personal_credential_only_session_omits_project_and_environment() {
        let client = reqwest::Client::new();
        let mut session = session_request(None);
        session.project_identifier = None;
        session.environment_identifier = None;
        session.bindings = vec![RemoteBinding {
            name: "LINEAR_API_KEY".to_owned(),
            source: RemoteBindingSource::PersonalCredential,
            source_name: "LINEAR_API_KEY".to_owned(),
            hosts: vec!["mcp.linear.app".to_owned()],
            rules: Vec::new(),
            header: "Authorization".to_owned(),
            placeholder: "${LINEAR_API_KEY}".to_owned(),
            value_template: "Bearer {value}".to_owned(),
        }];
        let request = create_session_http_request(&client, &session)
            .build()
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(request.body().and_then(reqwest::Body::as_bytes).unwrap())
                .unwrap();

        assert!(body.get("project_id").is_none());
        assert!(body.get("environment_id").is_none());
        assert_eq!(body["bindings"][0]["source"], "personal_credential");
    }

    #[test]
    fn session_request_serializes_secret_and_credential_bindings_together() {
        let client = reqwest::Client::new();
        let mut session = session_request(None);
        session.bindings = vec![
            RemoteBinding {
                name: "GITHUB_TOKEN".to_owned(),
                source: RemoteBindingSource::Secret,
                source_name: "GITHUB_TOKEN".to_owned(),
                hosts: vec!["api.github.com".to_owned()],
                rules: Vec::new(),
                header: "Authorization".to_owned(),
                placeholder: "{{GITHUB_TOKEN}}".to_owned(),
                value_template: "Bearer {value}".to_owned(),
            },
            RemoteBinding {
                name: "LINEAR_API_KEY".to_owned(),
                source: RemoteBindingSource::PersonalCredential,
                source_name: "LINEAR_API_KEY".to_owned(),
                hosts: vec!["mcp.linear.app".to_owned()],
                rules: Vec::new(),
                header: "Authorization".to_owned(),
                placeholder: "{{LINEAR_API_KEY}}".to_owned(),
                value_template: "Bearer {value}".to_owned(),
            },
        ];
        let request = create_session_http_request(&client, &session)
            .build()
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(request.body().and_then(reqwest::Body::as_bytes).unwrap())
                .unwrap();

        assert_eq!(body["bindings"][0]["source"], "secret");
        assert_eq!(body["bindings"][1]["source"], "personal_credential");
        assert_eq!(body["bindings"][1]["source_name"], "LINEAR_API_KEY");
    }

    #[test]
    fn replacement_session_omits_the_initial_agent_type() {
        let client = reqwest::Client::new();
        let initial = session_request(None);
        let replacement = initial.replacement("old-token".to_owned());
        let initial_request = create_session_http_request(&client, &initial)
            .build()
            .unwrap();
        let replacement_request = create_session_http_request(&client, &replacement)
            .build()
            .unwrap();
        let initial_body: serde_json::Value = serde_json::from_slice(
            initial_request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .unwrap(),
        )
        .unwrap();
        let replacement_body: serde_json::Value = serde_json::from_slice(
            replacement_request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .unwrap(),
        )
        .unwrap();

        assert_eq!(initial_body["agent_type"], "custom");
        assert!(replacement_body.get("agent_type").is_none());
        assert_eq!(
            replacement_request
                .headers()
                .get("X-Stashbase-Previous-Session")
                .unwrap(),
            "old-token"
        );
    }

    #[test]
    fn final_cleanup_request_marks_the_agent_run_ended() {
        let client = reqwest::Client::new();
        let request =
            delete_session_http_request(&client, "test-api-key".to_owned(), "current-token", true)
                .build()
                .unwrap();

        assert_eq!(request.method(), reqwest::Method::DELETE);
        assert_eq!(
            request.headers().get("X-Stashbase-Session").unwrap(),
            "current-token"
        );
        assert_eq!(
            request.headers().get("X-Stashbase-End-Agent-Run").unwrap(),
            "true"
        );
    }

    #[test]
    fn session_errors_use_the_standard_api_error_format() {
        let error = format_session_error(
            reqwest::StatusCode::FORBIDDEN,
            Some(ApiErrorResponse {
                error: crate::models::api_client::ApiError {
                    code: "agent_proxy.subscription_required".to_owned(),
                    message: Some(
                        "An active paid workspace subscription is required to use the Agent Proxy."
                            .to_owned(),
                    ),
                    hint: None,
                    details: None,
                },
            }),
            false,
            false,
        )
        .unwrap();

        assert!(error.contains("API Error (403)"));
        assert!(error.contains("Code: agent_proxy.subscription_required"));
        assert!(error.contains("Message: An active paid workspace subscription"));
        assert!(!error.contains("Hint:"));
    }

    #[test]
    fn credential_access_disabled_error_is_actionable() {
        let error = format_session_error(
            reqwest::StatusCode::FORBIDDEN,
            Some(ApiErrorResponse {
                error: crate::models::api_client::ApiError {
                    code: "agent_proxy.credential_access_disabled".to_owned(),
                    message: Some("Credential access is disabled for this workspace.".to_owned()),
                    hint: None,
                    details: None,
                },
            }),
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            error,
            "Enable Personal credential access in Workspace → Agents before starting this Remote Agent session."
        );
    }
}
