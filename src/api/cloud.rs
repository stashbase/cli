use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    api::client,
    models::api_client::{ApiErrorResponse, OutputError},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct CloudSession {
    pub id: String,
    pub status: String,
    pub isolation: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CloudAttach {
    pub tunnel_token: String,
    pub ssh_user: String,
}

/// No profile yet: the backend creates a plain sandbox without an Agent Proxy session.
#[derive(Serialize)]
struct CreateCloudSession<'a> {
    ssh_public_key: &'a str,
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()?)
}

async fn send(request: reqwest::RequestBuilder, json_format: bool) -> Result<reqwest::Response> {
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            let output_error = if error.is_timeout() {
                OutputError::request_timed_out()
            } else {
                OutputError::cannot_connect()
            };
            bail!(output_error.format_error_output(json_format)?);
        }
    };
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    match response.json::<ApiErrorResponse>().await {
        Ok(body) => {
            let error = OutputError::from(body.error).with_status(Some(status.as_u16()));
            bail!(error.format_error_output(json_format)?)
        }
        Err(_) => bail!("Request failed with status {status}."),
    }
}

fn url(path: &str) -> String {
    format!("{}{}", client::get_api_url(), path)
}

pub async fn create(
    api_key: &str,
    ssh_public_key: &str,
    json_format: bool,
) -> Result<CloudSession> {
    let body = CreateCloudSession { ssh_public_key };
    let request = http_client()?
        .post(url("/v1/cloud/sessions"))
        .bearer_auth(api_key)
        .json(&body);
    send(request, json_format)
        .await?
        .json()
        .await
        .context("invalid cloud session response")
}

pub async fn list(api_key: &str, json_format: bool) -> Result<Vec<CloudSession>> {
    let request = http_client()?
        .get(url("/v1/cloud/sessions"))
        .bearer_auth(api_key);
    send(request, json_format)
        .await?
        .json()
        .await
        .context("invalid cloud sessions response")
}

pub async fn attach(api_key: &str, id: &str, json_format: bool) -> Result<CloudAttach> {
    let request = http_client()?
        .post(url(&format!("/v1/cloud/sessions/{id}/attach")))
        .bearer_auth(api_key);
    send(request, json_format)
        .await?
        .json()
        .await
        .context("invalid cloud attach response")
}

pub async fn stop(api_key: &str, id: &str, json_format: bool) -> Result<()> {
    let request = http_client()?
        .post(url(&format!("/v1/cloud/sessions/{id}/stop")))
        .bearer_auth(api_key);
    send(request, json_format).await?;
    Ok(())
}
