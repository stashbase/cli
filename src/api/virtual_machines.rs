use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    api::client,
    models::api_client::{ApiErrorResponse, OutputError},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    /// creating, running, stopping, stopped, resuming, error or deleted.
    pub state: String,
    #[serde(default)]
    pub ip: Option<String>,
    /// `ssh agent@<host>` for a running machine; uses its stable name when the server has one.
    #[serde(default)]
    pub ssh_command: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub running_since: Option<String>,
    /// When the machine stops if it stays idle; absent while busy or stopped.
    #[serde(default)]
    pub idle_stop_at: Option<String>,
    /// False when idle auto-stop is turned off (absent means on).
    #[serde(default)]
    pub auto_stop_enabled: Option<bool>,
    /// The latest reading (about once a minute) while the machine runs.
    #[serde(default)]
    pub metrics: Option<Metrics>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub cpu_percent: f64,
    pub memory_used_bytes: f64,
    pub memory_total_bytes: f64,
    pub storage_used_bytes: f64,
    pub storage_total_bytes: f64,
    pub measured_at: String,
}

/// The workspace's pooled machine allowance for the current period.
#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub allowance_status: String,
    #[serde(default)]
    pub allowance: Option<Allowance>,
    #[serde(default)]
    pub machines: Option<MachineCount>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Allowance {
    /// `trial` (one-time) or `subscription` (resets every billing period).
    pub kind: String,
    #[serde(default)]
    pub ends_at: Option<String>,
    pub included_seconds: f64,
    pub used_seconds: f64,
    pub remaining_seconds: f64,
    #[serde(default)]
    pub overage_cap_seconds: f64,
    #[serde(default)]
    pub overage_used_seconds: f64,
    #[serde(default)]
    pub overage_cents: f64,
    #[serde(default)]
    pub included_egress_bytes: f64,
    #[serde(default)]
    pub used_egress_bytes: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MachineCount {
    pub current_count: u32,
    pub max_count: u32,
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
        let body: Data<T> = response
            .json()
            .await
            .context("invalid virtual machine response")?;
        return Ok(body.data);
    }
    let text = response.text().await.unwrap_or_default();
    if let Ok(body) = serde_json::from_str::<ApiErrorResponse>(&text) {
        let error = OutputError::from(body.error).with_status(Some(status.as_u16()));
        bail!(error.format_error_output(json_format)?)
    }
    if let Ok(PlainError { code, message }) = serde_json::from_str::<PlainError>(&text) {
        if let Some(text) = code.as_deref().and_then(error_message) {
            bail!("{text}")
        }
        bail!(
            "{} ({status})",
            code.or(message).unwrap_or_else(|| "request failed".into())
        )
    }
    bail!("Request failed with status {status}.")
}

/// Readable messages for the errors users run into, as in the dashboard. Unknown codes are
/// printed as they are.
fn error_message(code: &str) -> Option<&'static str> {
    Some(match code.strip_prefix("virtual_machines.")? {
        "two_factor_required" => "Creating a machine requires two-factor authentication. Turn it on in the dashboard: Account settings → Authentication.",
        "trial_not_claimed" => "Machines need the one-time free trial or a paid plan. Ask the workspace owner to claim the trial in the dashboard.",
        "allowance_exhausted" => "This workspace has used all its included machine time. The owner can allow extra hours on a paid plan, or upgrade.",
        "egress_exhausted" => "This workspace has used all its included data transfer for machines.",
        "plan_configuration_unavailable" => "Machines are not available for this workspace's plan. Contact support.",
        "member_limit" => "You have reached your machine limit. Stopped machines count; delete one with `stashbase vms delete <ID>`.",
        "not_invited" => "Your account is not enabled for virtual machines yet.",
        "feature_disabled" => "Virtual machines are not enabled.",
        "access_suspended" => "Machine access is suspended for this account or workspace.",
        "budget_limit" => "Machines are temporarily unavailable. Try again later.",
        "setup_not_ready" => "Virtual machines are not available yet. Try again later.",
        "operation_busy" => "Another operation is running on this machine. Try again shortly.",
        "invalid_transition" => "The machine cannot do that in its current state. Check it with `stashbase vms list`.",
        "provision_failed" => "The machine could not be started. Try again.",
        "host_key_unavailable" => "The machine is still starting. Try again in a minute.",
        "rate_limited" => "Too many requests. Wait a moment and try again.",
        "not_found" => "No such machine in this workspace.",
        _ => return None,
    })
}

fn url(path: &str) -> String {
    format!("{}/v1/virtual-machines{}", client::get_api_url(), path)
}

pub async fn create(
    api_key: &str,
    ssh_public_key: &str,
    location: Option<&str>,
    json: bool,
) -> Result<Machine> {
    let mut body = serde_json::json!({ "ssh_public_key": ssh_public_key });
    if let Some(location) = location {
        body["location"] = location.into();
    }
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

pub async fn usage(api_key: &str, json: bool) -> Result<Usage> {
    send(http_client()?.get(url("/usage")).bearer_auth(api_key), json).await
}

pub async fn set_auto_stop(api_key: &str, id: &str, enabled: bool, json: bool) -> Result<Machine> {
    send(
        http_client()?
            .put(url(&format!("/{id}/autostop")))
            .bearer_auth(api_key)
            .json(&serde_json::json!({ "enabled": enabled })),
        json,
    )
    .await
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

#[derive(Debug, Deserialize)]
pub struct Certificate {
    /// A short-lived SSH certificate for the public key, valid for this machine only.
    pub certificate: String,
    /// The machine's SSH host key, as it reported it, for strict host key checking.
    pub host_key: String,
    pub ip: String,
}

/// Asks the backend to certify this public key for a running machine.
pub async fn certificate(
    api_key: &str,
    id: &str,
    ssh_public_key: &str,
    json: bool,
) -> Result<Certificate> {
    let body = serde_json::json!({ "ssh_public_key": ssh_public_key });
    send(
        http_client()?
            .post(url(&format!("/{id}/certificate")))
            .bearer_auth(api_key)
            .json(&body),
        json,
    )
    .await
}
