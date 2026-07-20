use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::api::client;

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
    pub from: String,
    pub hosts: Vec<String>,
    pub header: String,
    pub placeholder: String,
    pub value_template: String,
}

/// Everything needed to issue a replacement session. This remains in memory for
/// the duration of one agent command only.
#[derive(Clone)]
pub struct RemoteBrokerSessionRequest {
    pub api_key: String,
    pub project_identifier: String,
    pub environment_identifier: String,
    /// Ordinary destinations the agent may reach without secret injection.
    /// Per-secret destinations remain in each binding's `hosts` field.
    pub egress_hosts: Vec<String>,
    pub deny_hosts: Vec<String>,
    pub bindings: Vec<RemoteBinding>,
    /// Sent only for the initial logical agent session. The control plane keeps
    /// that value when a replacement session is issued.
    pub agent_type: Option<String>,
    /// Present only while rotating an existing logical agent session.
    pub previous_session_token: Option<String>,
}

impl RemoteBrokerSessionRequest {
    pub fn replacement(&self, previous_session_token: String) -> Self {
        let mut request = self.clone();
        request.agent_type = None;
        request.previous_session_token = Some(previous_session_token);
        request
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoteBrokerSession {
    pub session_id: String,
    pub session_token: String,
    pub expires_at: String,
    pub proxy_url: String,
    pub protocol: String,
}

#[derive(Serialize)]
struct CreateSession<'a> {
    project_id: &'a str,
    environment_id: &'a str,
    egress_hosts: &'a [String],
    deny_hosts: &'a [String],
    bindings: &'a [RemoteBinding],
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_type: Option<&'a str>,
}

pub async fn create_session(request: &RemoteBrokerSessionRequest) -> Result<RemoteBrokerSession> {
    let client = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()?;
    let response = create_session_http_request(&client, request)
        .send()
        .await
        .context("failed to create remote broker session")?;
    if !response.status().is_success() {
        let status = response.status();
        let message = response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|body| {
                body.get("error")?
                    .get("message")?
                    .as_str()
                    .map(str::to_owned)
            });
        if let Some(message) = message {
            bail!("Remote broker session request failed with HTTP {status}: {message}");
        }
        bail!("Remote broker session request failed with HTTP {status}.");
    }
    let mut session: RemoteBrokerSession = response
        .json()
        .await
        .context("invalid remote broker session response")?;
    if session.proxy_url.starts_with('/') {
        session.proxy_url = format!("{}{}", client::get_api_url(), session.proxy_url);
    }
    Ok(session)
}

fn create_session_http_request(
    client: &reqwest::Client,
    request: &RemoteBrokerSessionRequest,
) -> reqwest::RequestBuilder {
    let mut session_request = client
        .post(format!(
            "{}/v1/remote-broker/sessions",
            client::get_api_url()
        ))
        .bearer_auth(&request.api_key)
        .json(&CreateSession {
            project_id: &request.project_identifier,
            environment_id: &request.environment_identifier,
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
    let client = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()
        .expect("remote broker revoke client configuration is valid");
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
            "{}/v1/remote-broker/sessions/current",
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
    let _ = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()
        .expect("remote broker retire client configuration is valid")
        .post(format!(
            "{}/v1/remote-broker/sessions/current/retire",
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

    fn session_request(previous_session_token: Option<&str>) -> RemoteBrokerSessionRequest {
        RemoteBrokerSessionRequest {
            api_key: "test-api-key".to_owned(),
            project_identifier: "project".to_owned(),
            environment_identifier: "environment".to_owned(),
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
            from: "GH_TOKEN".to_owned(),
            hosts: vec!["api.github.com".to_owned(), "github.com".to_owned()],
            header: "authorization".to_owned(),
            placeholder: "${STASHBASE_GH_TOKEN}".to_owned(),
            value_template: "Bearer {secret}".to_owned(),
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
}
