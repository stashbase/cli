use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::{human_datetime::get_human_datetime, output::ColorizeIfColoredOutput};

use super::shared::PaginationMetadata;

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct ListWebhook {
    #[tabled(rename = "ID", order = 0)]
    id: String,

    #[tabled(rename = "URL", order = 1)]
    url: String,

    #[tabled(rename = "Enabled", order = 2)]
    enabled: bool,
}

impl Display for ListWebhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.enabled == true {
            writeln!(f, "{} {}", "Enabled:".blue_bold_if_tty(), "true")?;
        } else {
            writeln!(f, "{} {}", "Enabled:".blue_bold_if_tty(), "false")?;
        }

        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;
        writeln!(f, "{} {}", "URL:".blue_bold_if_tty(), self.url)?;

        Ok(())
    }
}

// NOTE: with details
#[derive(Debug, Serialize, Deserialize)]
pub struct Webhook {
    pub id: String,
    pub url: String,
    pub enabled: bool,

    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct TableWebhook {
    #[tabled(rename = "ID", order = 0)]
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
pub struct TableWebhookNoDescription {
    #[tabled(rename = "ID", order = 0)]
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
            writeln!(f, "{} {}", "Enabled:".blue_bold_if_tty(), "true")?;
        } else {
            writeln!(f, "{} {}", "Enabled:".blue_bold_if_tty(), "false")?;
        }

        let (formatted, relative) = get_human_datetime(&self.created_at);
        writeln!(
            f,
            "{} {} ({})",
            "Created at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        writeln!(f, "{} {}", "URL:".blue_bold_if_tty(), self.url)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".blue_bold_if_tty(), description)?;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateWebhookResponse {
    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct TestWebhookResponse {
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

impl TestWebhookResponse {
    pub fn is_success(&self) -> bool {
        if let Some(status) = &self.status {
            *status == 200 || *status == 204
        } else {
            false
        }
    }
}

impl Display for TestWebhookResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_success() {
            writeln!(f, "{} {}", "Status:".blue_bold_if_tty(), "success")?;
            writeln!(
                f,
                "{} {}",
                "HTTP status code:".blue_bold_if_tty(),
                self.status.unwrap()
            )?;
            writeln!(
                f,
                "{} Wehbook event delivered",
                "Response message:".blue_bold_if_tty()
            )?;
            writeln!(f, "{} {}", "Webhook URL:".blue_bold_if_tty(), self.url)?;
        } else {
            writeln!(f, "{} {}", "Status:".blue_bold_if_tty(), "failure")?;

            if let Some(status) = &self.status {
                writeln!(f, "{} {}", "HTTP status code:".blue_bold_if_tty(), status)?;
                writeln!(
                    f,
                    "{} Failed with status code",
                    "Response message:".blue_bold_if_tty()
                )?;
            } else {
                writeln!(f, "{} N/A", "HTTP status code:".blue_bold_if_tty())?;
            }

            if let Some(error) = &self.error {
                writeln!(
                    f,
                    "{} {}",
                    "Response message:".blue_bold_if_tty(),
                    error.get_message()
                )?;
            } else {
                writeln!(
                    f,
                    "{} Unknown error",
                    "Response message:".blue_bold_if_tty()
                )?;
            }

            writeln!(f, "{} {}", "Webhook URL:".blue_bold_if_tty(), self.url)?;
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookSigningSecret {
    pub signing_secret: String,
}

// logs

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookLogList {
    pub data: Vec<WebhookLog>,
    pub pagination: PaginationMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
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
                writeln!(f, "{} {}", "Status:".blue_bold_if_tty(), "success")?;
            } else {
                writeln!(f, "{} {}", "Status:".blue_bold_if_tty(), "failure")?;
            }
        } else {
            writeln!(f, "{} {}", "Status:".blue_bold_if_tty(), "failure")?;
        }

        writeln!(
            f,
            "{} {}",
            "Attempt number:".blue_bold_if_tty(),
            self.attempt
        )?;

        if let Some(status) = &self.status {
            writeln!(f, "{} {}", "HTTP status code:".blue_bold_if_tty(), status)?;
        } else {
            writeln!(f, "{} {}", "HTTP status code:".blue_bold_if_tty(), "N/A")?;
        }

        if let Some(status) = self.status {
            if status == 200 || status == 204 {
                writeln!(
                    f,
                    "{} Wehbook event delivered",
                    "Response message:".blue_bold_if_tty()
                )?;
            } else {
                writeln!(
                    f,
                    "{} Failed with status code",
                    "Response message:".blue_bold_if_tty()
                )?;
            }
        } else {
            if let Some(error_code) = &self.error {
                writeln!(
                    f,
                    "{} {}",
                    "Response message:".blue_bold_if_tty(),
                    error_code.get_message()
                )?;
            } else {
                writeln!(
                    f,
                    "{} Unknown error",
                    "Response message:".blue_bold_if_tty()
                )?;
            }
        }

        // writeln!(f, "Attempt number: {}", self.attempt)?;
        let (formatted, relative) = get_human_datetime(&self.processed_at);
        writeln!(
            f,
            "{} {} ({})",
            "Processed at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

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
