use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    api::client,
    models::api_client::{ApiError, ApiErrorResponse, OutputError},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    /// creating, running, stopping, stopped, resuming, deleting, error or deleted.
    pub state: String,
    #[serde(default)]
    pub ip: Option<String>,
    /// `ssh agent@<host>` for a running machine; uses its stable name when the server has one.
    #[serde(default)]
    pub ssh_command: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Stopped long enough to be saved as a snapshot: starting it restores the disk (minutes).
    #[serde(default)]
    pub archived: Option<bool>,
    /// Running and accepting SSH connections (its host key reported and SSH answering).
    #[serde(default)]
    pub ready: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub running_since: Option<String>,
    /// When the machine suspends if it stays idle; absent while busy or not running.
    #[serde(default)]
    pub idle_suspend_at: Option<String>,
    /// Until when a suspended machine's session is kept; then it is shut down and archived.
    #[serde(default)]
    pub session_kept_until: Option<String>,
    /// False when auto-suspend is turned off (absent means on).
    #[serde(default)]
    pub auto_suspend_enabled: Option<bool>,
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
    /// The locations a machine can be created in (us, eu, asia), and the region of each.
    #[serde(default)]
    pub locations: Vec<String>,
    #[serde(default)]
    pub location_regions: std::collections::HashMap<String, String>,
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
    /// The most the owner allows to be billed past the included usage (hours and traffic), cents.
    #[serde(default)]
    pub overage_cap_cents: f64,
    #[serde(default)]
    pub overage_used_seconds: f64,
    /// What the extra hours and traffic so far cost, in USD cents.
    #[serde(default)]
    pub overage_cents: f64,
    #[serde(default)]
    pub egress_overage_bytes: f64,
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
        .timeout(Duration::from_secs(client::get_request_timeout_secs()))
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
            // The network error stays underneath, so callers can tell "no answer" from a refusal.
            return Err(
                anyhow::Error::new(error).context(output_error.format_error_output(json_format)?)
            );
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
    if let Ok(mut body) = serde_json::from_str::<ApiErrorResponse>(&text) {
        // The API's message, unless the CLI has a more specific one for this code.
        if let Some(message) = error_message(&body.error.code) {
            body.error.message = Some(message.to_owned());
        }
        let error = OutputError::from(body.error).with_status(Some(status.as_u16()));
        bail!(error.format_error_output(json_format)?)
    }
    if let Ok(PlainError { code, message }) = serde_json::from_str::<PlainError>(&text) {
        // Formatted like every other API error, with a readable message for known codes.
        let code = code.unwrap_or_else(|| "server.request_failed".to_owned());
        let error = ApiError {
            message: error_message(&code).map(str::to_owned).or(message),
            code,
            hint: None,
            details: None,
        };
        let error = OutputError::from(error).with_status(Some(status.as_u16()));
        bail!(error.format_error_output(json_format)?)
    }
    bail!("Request failed with status {status}.")
}

/// Readable messages for the errors users run into, as in the dashboard. Unknown codes are
/// printed as they are.
fn error_message(code: &str) -> Option<&'static str> {
    Some(match code {
        "access.missing_api_key_permission" => "This API key can't manage virtual machines. Create a personal API key with \"Manage virtual machines\" turned on in the dashboard.",
        "access.two_factor_required" => "Creating a machine requires two-factor authentication. Turn it on in the dashboard: Account settings → Authentication.",
        "conflict.trial_not_claimed" => "Machines need the one-time free trial or a paid plan. Ask the workspace owner to claim the trial in the dashboard.",
        "quota.virtual_machine_hours_exhausted" => "This workspace has used all its included machine time. The owner can allow extra hours on a paid plan, or upgrade.",
        "quota.virtual_machine_egress_exhausted" => "This workspace has used all its included data transfer for machines.",
        "quota.virtual_machine_overage_cap_reached" => "This workspace reached its extra usage cap for this billing period. The workspace owner can raise it in the dashboard; run `stashbase vms usage` to see what was used.",
        "server.virtual_machines_unavailable" => "Virtual machines are temporarily unavailable. Try again later.",
        "quota.virtual_machine_limit_reached" => "You have reached your machine limit. Stopped machines count; delete one with `stashbase vms delete <ID>`.",
        "access.virtual_machines_not_invited" => "Your account is not enabled for virtual machines yet.",
        "resource.virtual_machines_not_available" => "Virtual machines are not enabled.",
        "access.virtual_machines_suspended" => "Machine access is suspended for this account or workspace.",
        "quota.virtual_machine_usage_limit_reached" => "Machines are temporarily unavailable. Try again later.",
        "conflict.virtual_machine_not_ready" => "Virtual machines are not available yet. Try again later.",
        "conflict.virtual_machine_busy" => "Another operation is running on this machine. Try again shortly.",
        "conflict.invalid_virtual_machine_state" => "The machine cannot do that in its current state. Check it with `stashbase vms list`.",
        "server.virtual_machine_provision_failed" => "The machine could not be started. Try again.",
        "server.host_key_unavailable" => "The machine is still starting. Try again in a minute.",
        "rate_limit.too_many_requests" => "Too many requests. Wait a moment and try again.",
        "resource.virtual_machine_not_found" => "No such machine in this workspace.",
        "validation.invalid_virtual_machine_name" => "Use letters, numbers, spaces, dots, dashes or underscores for the name, up to 40 characters.",
        _ => return None,
    })
}

/// An error the CLI detects itself, formatted like the API's own errors.
fn local_error(code: &str, message: String, status: u16, json_format: bool) -> anyhow::Error {
    let error = ApiError {
        code: code.to_owned(),
        message: Some(message),
        hint: None,
        details: None,
    };
    match OutputError::from(error)
        .with_status(Some(status))
        .format_error_output(json_format)
    {
        Ok(text) => anyhow::anyhow!(text),
        Err(error) => error.into(),
    }
}

/// The not-found error for a machine missing from the list.
pub fn not_found(json_format: bool) -> anyhow::Error {
    let code = "resource.virtual_machine_not_found";
    let message = error_message(code).unwrap_or("No such machine in this workspace.");
    local_error(code, message.to_owned(), 404, json_format)
}

/// A machine that is changing state (creating, stopping, resuming, deleting) cannot connect yet.
pub fn busy(id: &str, state: &str, json_format: bool) -> anyhow::Error {
    local_error(
        "conflict.virtual_machine_busy",
        format!("Virtual machine {id} is {state}; try again shortly."),
        409,
        json_format,
    )
}

fn url(path: &str) -> String {
    format!("{}/v1/virtual-machines{}", client::get_api_url(), path)
}

pub async fn create(
    api_key: &str,
    ssh_public_key: &str,
    location: &str,
    name: Option<&str>,
    json: bool,
) -> Result<Machine> {
    let mut body = serde_json::json!({ "ssh_public_key": ssh_public_key, "location": location });
    if let Some(name) = name {
        body["name"] = name.into();
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

pub async fn set_auto_suspend(
    api_key: &str,
    id: &str,
    enabled: bool,
    json: bool,
) -> Result<Machine> {
    send(
        http_client()?
            .put(url(&format!("/{id}/autosuspend")))
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
