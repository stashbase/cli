use owo_colors::OwoColorize;
use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::human_datetime::get_human_datetime;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ListWebhook {
    #[tabled(order = 0)]
    id: String,

    #[tabled(order = 1)]
    url: String,

    #[tabled(order = 2)]
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
    url: String,
    enabled: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    created_at: String,
    // created_by: string
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
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookResponse {
    pub id: String,
}

// update
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateWebhookPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
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
    pub error_code: Option<TestWebhookErrorCode>,
}

#[derive(Debug, Deserialize)]
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
            TestWebhookErrorCode::ECONNABORTED => "Requer timed out".to_string(),
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
        writeln!(f, "Webhook URL: {}", self.url)?;

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

        Ok(())
    }
}

impl Display for TestWebhookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Webhook URL: {}", self.url)?;
        writeln!(f, "Status: {}", "failure".red())?;

        if let Some(status) = &self.status {
            writeln!(f, "HTTP status code: {}", status)?;
            writeln!(f, "Response message: Failed with status code")?;
        } else {
            writeln!(f, "HTTP status code: N/A")?;

            if let Some(error_code) = &self.error_code {
                writeln!(f, "Response message: {}", error_code.get_message())?;
            } else {
                writeln!(f, "Response message: Unknown error")?;
            }
        }

        Ok(())
    }
}
