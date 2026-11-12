use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    api::client,
    models::api_client::{ApiErrorResponse, OutputError},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub id: String,
    /// creating, running, stopping, stopped, resuming, error or deleted.
    pub state: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetupError {
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetupStatus {
    pub required: bool,
    /// unconfigured, preparing, ready or error.
    pub state: String,
    #[serde(default)]
    pub error: Option<SetupError>,
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
struct PlainError {
    code: Option<String>,
    message: Option<String>,
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(client::CLI_USER_AGENT)
        .build()?)
}

async fn send<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    json_format: bool,
) -> Result<T> {
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
    let status = response.status();
    if status.is_success() {
        let body: Data<T> = response.json().await.context("invalid cloud response")?;
        return Ok(body.data);
    }
    let text = response.text().await.unwrap_or_default();
    if let Ok(body) = serde_json::from_str::<ApiErrorResponse>(&text) {
        let error = OutputError::from(body.error).with_status(Some(status.as_u16()));
        bail!(error.format_error_output(json_format)?)
    }
    if let Ok(PlainError { code, message }) = serde_json::from_str::<PlainError>(&text) {
        bail!(
            "{} ({status})",
            code.or(message).unwrap_or_else(|| "request failed".into())
        )
    }
    bail!("Request failed with status {status}.")
}

fn url(path: &str) -> String {
    format!("{}/v1/cloud/machines{}", client::get_api_url(), path)
}

pub async fn setup_status(api_key: &str, json: bool) -> Result<SetupStatus> {
    send(http_client()?.get(url("/setup")).bearer_auth(api_key), json).await
}

pub async fn setup_start(api_key: &str, rebuild: bool, json: bool) -> Result<SetupStatus> {
    let path = if rebuild {
        "/setup?rebuild=true"
    } else {
        "/setup"
    };
    send(http_client()?.post(url(path)).bearer_auth(api_key), json).await
}

pub async fn set_profile(api_key: &str, toml: &str, json: bool) -> Result<serde_json::Value> {
    let body = serde_json::json!({ "toml": toml });
    send(
        http_client()?
            .put(url("/profile"))
            .bearer_auth(api_key)
            .json(&body),
        json,
    )
    .await
}

pub async fn create(api_key: &str, ssh_public_key: &str, json: bool) -> Result<Machine> {
    let body = serde_json::json!({ "sshPublicKey": ssh_public_key });
    send(
        http_client()?
            .post(url(""))
            .bearer_auth(api_key)
            .json(&body),
        json,
    )
    .await
}

pub async fn list(api_key: &str, json: bool) -> Result<Vec<Machine>> {
    send(http_client()?.get(url("")).bearer_auth(api_key), json).await
}

pub async fn resume(api_key: &str, id: &str, json: bool) -> Result<Machine> {
    send(
        http_client()?
            .post(url(&format!("/{id}/resume")))
            .bearer_auth(api_key),
        json,
    )
    .await
}

pub async fn stop(api_key: &str, id: &str, json: bool) -> Result<Machine> {
    send(
        http_client()?
            .post(url(&format!("/{id}/stop")))
            .bearer_auth(api_key),
        json,
    )
    .await
}

pub async fn destroy(api_key: &str, id: &str, json: bool) -> Result<Machine> {
    send(
        http_client()?
            .delete(url(&format!("/{id}")))
            .bearer_auth(api_key),
        json,
    )
    .await
}
