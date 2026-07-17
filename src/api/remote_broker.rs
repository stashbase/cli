use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::client;

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
    pub allowed_hosts: Vec<String>,
    pub deny_hosts: Vec<String>,
    pub bindings: Vec<RemoteBinding>,
    /// Present only while rotating an existing logical agent session.
    pub previous_session_token: Option<String>,
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
    allowed_hosts: &'a [String],
    deny_hosts: &'a [String],
    bindings: &'a [RemoteBinding],
}

pub async fn create_session(request: &RemoteBrokerSessionRequest) -> Result<RemoteBrokerSession> {
    let url = format!("{}/v1/remote-broker/sessions", client::get_api_url());
    let client = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()?;
    let mut session_request = client
        .post(url)
        .bearer_auth(&request.api_key)
        .json(&CreateSession {
            project_id: &request.project_identifier,
            environment_id: &request.environment_identifier,
            allowed_hosts: &request.allowed_hosts,
            deny_hosts: &request.deny_hosts,
            bindings: &request.bindings,
        });
    if let Some(previous_session_token) = &request.previous_session_token {
        session_request =
            session_request.header("X-Stashbase-Previous-Session", previous_session_token);
    }
    let response = session_request
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

pub async fn revoke_session(api_key: String, session_token: &str) {
    let _ = reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()
        .expect("remote broker revoke client configuration is valid")
        .delete(format!(
            "{}/v1/remote-broker/sessions/current",
            client::get_api_url()
        ))
        .bearer_auth(api_key)
        .header("X-Stashbase-Session", session_token)
        .send()
        .await;
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
