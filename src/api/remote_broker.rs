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

#[derive(Debug, Deserialize)]
pub struct RemoteBrokerSession {
    pub session_token: String,
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
    ttl_seconds: u16,
}

pub async fn create_session(
    api_key: String,
    project_identifier: String,
    environment_identifier: String,
    allowed_hosts: Vec<String>,
    deny_hosts: Vec<String>,
    bindings: Vec<RemoteBinding>,
) -> Result<RemoteBrokerSession> {
    let url = format!("{}/v1/remote-broker/sessions", client::get_api_url());
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(&api_key)
        .json(&CreateSession {
            project_id: &project_identifier,
            environment_id: &environment_identifier,
            allowed_hosts: &allowed_hosts,
            deny_hosts: &deny_hosts,
            bindings: &bindings,
            ttl_seconds: 600,
        })
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
    let _ = reqwest::Client::new()
        .delete(format!(
            "{}/v1/remote-broker/sessions/current",
            client::get_api_url()
        ))
        .bearer_auth(api_key)
        .header("X-Stashbase-Session", session_token)
        .send()
        .await;
}
