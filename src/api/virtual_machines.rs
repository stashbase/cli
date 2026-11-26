use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    api::client,
    models::api_client::{ApiError, ApiErrorResponse, GenericOutputError, OutputError},
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

/// True once the user pressed Ctrl+C.
pub fn aborted() -> bool {
    crate::REQUEST_ABORTED.load(std::sync::atomic::Ordering::SeqCst)
}

async fn wait_for_abort() {
    while !aborted() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn aborted_error(json_format: bool) -> anyhow::Error {
    match OutputError::request_aborted().format_error_output(json_format) {
        Ok(message) => anyhow::anyhow!(message),
        Err(error) => error.into(),
    }
}

/// Fails once the user pressed Ctrl+C, so nothing more is started after a cancel.
pub fn ensure_not_aborted(json_format: bool) -> Result<()> {
    if aborted() {
        return Err(aborted_error(json_format));
    }
    Ok(())
}

/// Waits between polls, ending early with the cancel error when the user presses Ctrl+C.
pub async fn pause(duration: Duration, json_format: bool) -> Result<()> {
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(()),
        _ = wait_for_abort() => Err(aborted_error(json_format)),
    }
}

/// The request got no answer (timeout or no connection). Callers can tell it from a refusal
/// by type; it prints only its formatted message, with no cause chain, so `--json` output stays
/// one JSON document.
#[derive(Debug)]
pub struct NoAnswer(String);

impl std::fmt::Display for NoAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoAnswer {}

/// The API answered with an error status. Prints only its formatted message, like `NoAnswer`.
#[derive(Debug)]
pub struct ApiRefusal {
    pub status: u16,
    message: String,
}

impl std::fmt::Display for ApiRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiRefusal {}

/// Worth retrying while waiting on a machine: no answer, or a server error or rate limit. A
/// refusal such as a revoked key or a machine you no longer have access to is not.
pub fn is_transient(error: &anyhow::Error) -> bool {
    error.downcast_ref::<NoAnswer>().is_some()
        || error
            .downcast_ref::<ApiRefusal>()
            .is_some_and(|refusal| refusal.status >= 500 || refusal.status == 429)
}

/// A success whose body could not be read (the connection dropped mid-answer, or the body is
/// not what was expected): the request may well have done its work, so it counts as no answer.
fn unreadable_answer(json_format: bool) -> anyhow::Error {
    let error = OutputError::Generic(GenericOutputError {
        code: Some("response.unreadable".to_owned()),
        message: "The API's answer could not be read.".to_owned(),
        status: None,
        hint: None,
        details: None,
    });
    match error.format_error_output(json_format) {
        Ok(message) => NoAnswer(message).into(),
        Err(error) => error.into(),
    }
}

async fn send<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    json_format: bool,
) -> Result<T> {
    // Like the rest of the CLI: no request starts after Ctrl+C, and one in flight is dropped,
    // including while its body is still being read.
    ensure_not_aborted(json_format)?;
    tokio::select! {
        result = send_and_read(request, json_format) => result,
        _ = wait_for_abort() => Err(aborted_error(json_format)),
    }
}

async fn send_and_read<T: serde::de::DeserializeOwned>(
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
            return Err(NoAnswer(output_error.format_error_output(json_format)?).into());
        }
    };
    let status = response.status();
    if status.is_success() {
        let Ok(bytes) = response.bytes().await else {
            return Err(unreadable_answer(json_format));
        };
        return match serde_json::from_slice::<Data<T>>(&bytes) {
            Ok(body) => Ok(body.data),
            Err(_) => Err(unreadable_answer(json_format)),
        };
    }
    let refusal = |message: String| -> anyhow::Error {
        ApiRefusal {
            status: status.as_u16(),
            message,
        }
        .into()
    };
    let text = response.text().await.unwrap_or_default();
    if let Ok(mut body) = serde_json::from_str::<ApiErrorResponse>(&text) {
        // The API's message, unless the CLI has a more specific one for this code.
        if let Some(message) = error_message(&body.error.code) {
            body.error.message = Some(message.to_owned());
        }
        let error = OutputError::from(body.error).with_status(Some(status.as_u16()));
        return Err(refusal(error.format_error_output(json_format)?));
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
        return Err(refusal(error.format_error_output(json_format)?));
    }
    Err(refusal(format!("Request failed with status {status}.")))
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

/// The body of a create request. The key is always empty: the machine gets no login key and is
/// reached only with certificates from `certificate`, so no `authorized_keys` entry outlives a
/// revoked API key or a disabled 2FA.
fn create_body(location: &str, name: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({ "ssh_public_key": "", "location": location });
    if let Some(name) = name {
        body["name"] = name.into();
    }
    body
}

pub async fn create(
    api_key: &str,
    location: &str,
    name: Option<&str>,
    json: bool,
) -> Result<Machine> {
    let body = create_body(location, name);
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

/// Renames a machine; `None` clears its name.
pub async fn rename(api_key: &str, id: &str, name: Option<&str>, json: bool) -> Result<Machine> {
    send(
        http_client()?
            .patch(url(&format!("/{id}")))
            .bearer_auth(api_key)
            .json(&serde_json::json!({ "name": name })),
        json,
    )
    .await
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

/// Restarts a running machine's idle countdown (`connect --keep-awake`).
pub async fn keep_awake(api_key: &str, id: &str, json: bool) -> Result<Machine> {
    send(
        http_client()?
            .post(url(&format!("/{id}/keep-awake")))
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

#[cfg(test)]
mod tests {
    use super::{create_body, is_transient, ApiRefusal, NoAnswer};

    #[test]
    fn only_missing_answers_server_errors_and_rate_limits_are_retried() {
        let refusal = |status| {
            anyhow::Error::from(ApiRefusal {
                status,
                message: String::new(),
            })
        };
        assert!(is_transient(&anyhow::Error::from(NoAnswer(String::new()))));
        assert!(is_transient(&refusal(503)));
        assert!(is_transient(&refusal(429)));
        assert!(!is_transient(&refusal(401)));
        assert!(!is_transient(&refusal(404)));
        assert!(!is_transient(&anyhow::anyhow!("anything else")));
    }

    #[test]
    fn create_never_sends_a_login_key() {
        assert_eq!(
            create_body("eu", Some("dev")),
            serde_json::json!({ "ssh_public_key": "", "location": "eu", "name": "dev" })
        );
        assert_eq!(
            create_body("us", None),
            serde_json::json!({ "ssh_public_key": "", "location": "us" })
        );
    }

    #[test]
    fn a_missing_answer_prints_only_its_message() {
        let json = r#"{"error":{"code":"request.timed_out"}}"#;
        let error = anyhow::Error::from(NoAnswer(json.to_owned()));
        assert_eq!(format!("{error:?}"), json);
        assert!(error.downcast_ref::<NoAnswer>().is_some());
    }
}
