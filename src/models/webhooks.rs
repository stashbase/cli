use owo_colors::OwoColorize;
use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::human_datetime::get_human_datetime;

use super::shared::PaginationMetadata;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ListWebhook {
    #[tabled(rename = "Id", order = 0)]
    id: String,

    #[tabled(rename = "URL", order = 1)]
    url: String,

    #[tabled(rename = "Enabled", order = 2)]
    enabled: bool,
}

impl Display for ListWebhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.enabled == true {
            writeln!(f, "{} {}", "Enabled:", "true".green())?;
        } else {
            writeln!(f, "{} {}", "Enabled:", "false".red())?;
        }

        writeln!(f, "{} {}", "Id:", self.id)?;
        writeln!(f, "{} {}", "URL:", self.url)?;

        Ok(())
    }
}

// NOTE: with details
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: String,
    pub url: String,
    pub enabled: bool,

    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct TableWebhook {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Enabled", order = 1)]
    pub enabled: bool,

    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    #[tabled(rename = "URL", order = 3)]
    pub url: String,

    #[tabled(rename = "Description", order = 4)]
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct TableWebhookNoDescription {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Enabled", order = 1)]
    pub enabled: bool,

    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    #[tabled(rename = "URL", order = 3)]
    pub url: String,
}

impl From<Webhook> for TableWebhook {
    fn from(webhook: Webhook) -> Self {
        Self {
            id: webhook.id,
            url: webhook.url,
            enabled: webhook.enabled,
            created_at: webhook.created_at,
            description: webhook
                .description
                .unwrap_or_else(|| "".to_string())
                .to_string(),
        }
    }
}

impl From<Webhook> for TableWebhookNoDescription {
    fn from(webhook: Webhook) -> Self {
        Self {
            id: webhook.id,
            url: webhook.url,
            enabled: webhook.enabled,
            created_at: webhook.created_at,
        }
    }
}

impl Display for Webhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.enabled == true {
            writeln!(f, "{} {}", "Enabled:", "true".green())?;
        } else {
            writeln!(f, "{} {}", "Enabled:", "false".red())?;
        }

        let (formatted, relative) = get_human_datetime(&self.created_at);
        writeln!(f, "{} {} ({})", "Created at:", formatted, relative)?;

        writeln!(f, "{} {}", "URL:", self.url)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:", description)?;
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateWebhookPayload {
    pub url: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    // whether or not webhook should be enabled
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookResponse {
    pub id: String,

    pub signing_secret: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateWebhookSecretResponse {
    pub signing_secret: String,
}

// update
#[derive(Debug, Serialize)]
pub struct UpdateWebhookPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    // NOTE: created separate
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub enabled: Option<bool>,
}

// update
#[derive(Debug, Serialize)]
pub struct UpdateWebhookStatusPayload {
    pub enabled: bool,
}

// test
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TestWebhookResponse {
    Ok(TestWebhookOk),
    Err(TestWebhookError),
}

// OK response without checking status
// status must be 200 or 204
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestWebhookOk {
    pub url: String,
    pub status: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestWebhookError {
    pub url: String,

    pub status: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<TestWebhookErrorCode>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TestWebhookErrorCode {
    ECONNABORTED,
    ENOTFOUND,
    ECONNREFUSED,
    ETIMEDOUT,
    ECONNRESET,
    ENETURNEACH,
    ENHOSTUNREACH,
    EPROTO,
}

impl Display for TestWebhookResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestWebhookResponse::Err(err_res) => write!(f, "{}", err_res),
            TestWebhookResponse::Ok(ok_res) => write!(f, "{}", ok_res),
        }?;

        Ok(())
    }
}

impl TestWebhookErrorCode {
    pub fn get_message(&self) -> String {
        match self {
            TestWebhookErrorCode::ECONNABORTED => "Request timed out".to_string(),
            TestWebhookErrorCode::ENOTFOUND => "Unable to resolve server's DNS".to_string(),
            TestWebhookErrorCode::ECONNREFUSED => "Unable to connect to the server".to_string(),
            TestWebhookErrorCode::ETIMEDOUT => "Request timed out".to_string(),
            TestWebhookErrorCode::ECONNRESET => "Connection was reset unexpectedly".to_string(),
            TestWebhookErrorCode::ENETURNEACH => "Network is unreachable".to_string(),
            TestWebhookErrorCode::ENHOSTUNREACH => "Host is unreachable".to_string(),
            TestWebhookErrorCode::EPROTO => "Protocol error".to_string(),
        }
    }
}

impl Display for TestWebhookOk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // writeln!(f, "Webhook URL: {}", self.url)?;

        if self.status == 200 || self.status == 204 {
            writeln!(f, "Status: {}", "success".green())?;
        } else {
            writeln!(f, "Status: {}", "failure".red())?;
        }

        writeln!(f, "HTTP status code: {}", self.status)?;

        if self.status == 200 || self.status == 204 {
            writeln!(f, "Response message: Wehbook event delivered")?;
        } else {
            writeln!(f, "Response message: Failed with status code")?;
        }

        writeln!(f, "Webhook URL: {}", self.url)?;

        Ok(())
    }
}

impl Display for TestWebhookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // writeln!(f, "Webhook URL: {}", self.url)?;
        writeln!(f, "Status: {}", "failure".red())?;

        if let Some(status) = &self.status {
            writeln!(f, "HTTP status code: {}", status)?;
            writeln!(f, "Response message: Failed with status code")?;
        } else {
            writeln!(f, "HTTP status code: N/A")?;

            if let Some(error) = &self.error {
                writeln!(f, "Response message: {}", error.get_message())?;
            } else {
                writeln!(f, "Response message: Unknown error")?;
            }
        }

        writeln!(f, "Webhook URL: {}", self.url)?;

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSigningSecret {
    pub signing_secret: String,
}

// logs

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookLogList {
    pub data: Vec<WebhookLog>,
    pub pagination: PaginationMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookLog {
    pub processed_at: String,
    pub attempt: u8,

    // http status code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<TestWebhookErrorCode>,
}

#[derive(Debug, Tabled)]
pub struct TableWebhookLog {
    #[tabled(rename = "Status", order = 0)]
    pub status: Status,

    #[tabled(order = 1, rename = "Mesage")]
    pub response_message: String,

    #[tabled(order = 2, rename = "HTTP status")]
    pub http_status_code: String,

    #[tabled(order = 3)]
    pub attempt: u8,

    #[tabled(order = 4, rename = "Processed")]
    pub processed_at: String,
}

#[derive(Debug, Tabled)]
pub enum Status {
    Success,
    Failure,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Success => write!(f, "{}", "success"),
            Status::Failure => write!(f, "{}", "failure"),
        }
    }
}

impl Display for WebhookLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(status) = self.status {
            if status == 200 || status == 204 {
                writeln!(f, "Status: {}", "success".green())?;
                // writeln!(f, "Response message: Wehbook event delivered")?;
            } else {
                writeln!(f, "Status: {}", "failure".red())?;
                // writeln!(f, "Response message: Failed with status code")?;
            }
        } else {
            writeln!(f, "Status: {}", "failure".red())?;
            // if let Some(error_code) = &self.error {
            //     writeln!(f, "Response message: {}", error_code.get_message())?;
            // } else {
            //     writeln!(f, "Response message: Unknown error")?;
            // }
        }

        writeln!(f, "Attempt number: {}", self.attempt)?;

        if let Some(status) = &self.status {
            writeln!(f, "{} {}", "HTTP status code:", status)?;
        } else {
            writeln!(f, "{} {}", "HTTP status code:", "N/A")?;
        }

        if let Some(status) = self.status {
            if status == 200 || status == 204 {
                writeln!(f, "Response message: Wehbook event delivered")?;
            } else {
                writeln!(f, "Response message: Failed with status code")?;
            }
        } else {
            if let Some(error_code) = &self.error {
                writeln!(f, "Response message: {}", error_code.get_message())?;
            } else {
                writeln!(f, "Response message: Unknown error")?;
            }
        }

        // writeln!(f, "Attempt number: {}", self.attempt)?;
        let (formatted, relative) = get_human_datetime(&self.processed_at);
        writeln!(f, "{} {} ({})", "Processed at:", formatted, relative)?;

        Ok(())
    }
}

impl Display for WebhookLogList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list_string = self
            .data
            .iter()
            // reverse
            .rev()
            .map(|item| format!("{}", item))
            .collect::<Vec<String>>()
            .join("\n");

        writeln!(f, "{}", list_string)?;
        writeln!(f, "{}", self.pagination)?;

        // let page = self.page.unwrap_or(1);
        //
        // if self.pages == 0 {
        //     writeln!(f, "No changes")?;
        // } else {
        //     writeln!(f, "{} {}/{}", "Pages:", page, self.pages)?;
        // }
        //
        Ok(())
    }
}

impl From<WebhookLog> for TableWebhookLog {
    fn from(log: WebhookLog) -> Self {
        // Success/Failure, not http status
        let status = match log.status {
            Some(status) => match status {
                200 | 204 => Status::Success,
                _ => Status::Failure,
            },
            None => Status::Failure,
        };

        let http_status_code = match log.status {
            Some(status) => status.to_string(),
            None => "N/A".to_string(),
        };

        let response_message = if let Some(status) = log.status {
            if status == 200 || status == 204 {
                "Wehbook event delivered".to_string()
            } else {
                "Failed with status code".to_string()
            }
        } else {
            if let Some(error_code) = &log.error {
                error_code.get_message()
            } else {
                "Unknown error".to_string()
            }
        };

        Self {
            processed_at: log.processed_at,
            status,
            attempt: log.attempt,
            response_message,
            http_status_code,
        }
    }
}
