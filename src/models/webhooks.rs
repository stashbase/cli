use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::{human_datetime::get_human_datetime, output::ColorizeIfColoredOutput};

use super::shared::PaginationMetadata;

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct ListWebhook {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "URL", order = 1)]
    pub url: String,

    #[tabled(rename = "Enabled", order = 2)]
    pub enabled: bool,

    #[tabled(rename = "Created", order = 3)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 4)]
    pub updated_at: String,
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

        let (formatted, relative) = get_human_datetime(&self.created_at);
        writeln!(
            f,
            "{} {} ({})",
            "Created at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        if self.created_at != self.updated_at {
            let (formatted_updated, relative_updated) = get_human_datetime(&self.updated_at);
            writeln!(
                f,
                "{} {} ({})",
                "Updated at:".blue_bold_if_tty(),
                formatted_updated,
                relative_updated
            )?;
        }

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
    pub updated_at: String,

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

    #[tabled(rename = "URL", order = 2)]
    pub url: String,

    #[tabled(rename = "Description", order = 3)]
    pub description: String,

    #[tabled(rename = "Created", order = 4)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 5)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct TableWebhookNoDescription {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Enabled", order = 1)]
    pub enabled: bool,

    #[tabled(rename = "URL", order = 2)]
    pub url: String,

    #[tabled(rename = "Created", order = 3)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 4)]
    pub updated_at: String,
}

impl From<Webhook> for TableWebhook {
    fn from(webhook: Webhook) -> Self {
        let (formatted_created, _) = get_human_datetime(&webhook.created_at);
        let (formatted_updated, _) = get_human_datetime(&webhook.updated_at);

        Self {
            id: webhook.id,
            url: webhook.url,
            enabled: webhook.enabled,
            created_at: formatted_created,
            updated_at: formatted_updated,
            description: webhook
                .description
                .unwrap_or_else(|| "".to_string())
                .to_string(),
        }
    }
}

impl From<Webhook> for TableWebhookNoDescription {
    fn from(webhook: Webhook) -> Self {
        let (formatted_created, _) = get_human_datetime(&webhook.created_at);
        let (formatted_updated, _) = get_human_datetime(&webhook.updated_at);

        Self {
            id: webhook.id,
            url: webhook.url,
            enabled: webhook.enabled,
            created_at: formatted_created,
            updated_at: formatted_updated,
        }
    }
}

impl Display for Webhook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;
        writeln!(f, "{} {}", "URL:".blue_bold_if_tty(), self.url)?;
        if self.enabled == true {
            writeln!(f, "{} {}", "Enabled:".blue_bold_if_tty(), "true")?;
        } else {
            writeln!(f, "{} {}", "Enabled:".blue_bold_if_tty(), "false")?;
        }

        if let Some(signing_secret) = &self.signing_secret {
            writeln!(
                f,
                "{} {}",
                "Signing secret:".blue_bold_if_tty(),
                signing_secret
            )?;
        }

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".blue_bold_if_tty(), description)?;
        }

        let (formatted, relative) = get_human_datetime(&self.created_at);
        writeln!(
            f,
            "{} {} ({})",
            "Created at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        if self.created_at != self.updated_at {
            let (formatted_updated, relative_updated) = get_human_datetime(&self.updated_at);
            writeln!(
                f,
                "{} {} ({})",
                "Updated at:".blue_bold_if_tty(),
                formatted_updated,
                relative_updated
            )?;
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
    pub response_body: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
            if let Some(response_body) = &self.response_body {
                write_response_body(f, response_body)?;
            }
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
                    get_test_webhook_error_message(error)
                )?;
            } else {
                writeln!(
                    f,
                    "{} Unknown error",
                    "Response message:".blue_bold_if_tty()
                )?;
            }

            writeln!(f, "{} {}", "Webhook URL:".blue_bold_if_tty(), self.url)?;
            if let Some(response_body) = &self.response_body {
                write_response_body(f, response_body)?;
            }
        }

        Ok(())
    }
}

fn get_test_webhook_error_message(error: &str) -> &str {
    match error {
        "ECONNABORTED" => "Request timed out",
        "ENOTFOUND" => "Unable to resolve server's DNS",
        "ECONNREFUSED" => "Unable to connect to the server",
        "ETIMEDOUT" => "Request timed out",
        "ECONNRESET" => "Connection was reset unexpectedly",
        "ENETUNREACH" => "Network is unreachable",
        "ENHOSTUNREACH" => "Host is unreachable",
        "EPROTO" => "Protocol error",
        _ => "Unknown error",
    }
}

fn format_response_body(response_body: &str) -> &str {
    if response_body.is_empty() {
        "\"\""
    } else {
        response_body
    }
}

fn write_response_body(f: &mut std::fmt::Formatter<'_>, response_body: &str) -> std::fmt::Result {
    if response_body.is_empty() {
        writeln!(
            f,
            "{} {}",
            "Response body:".blue_bold_if_tty(),
            format_response_body(response_body)
        )
    } else {
        writeln!(f, "{}", "Response body:".blue_bold_if_tty())?;
        writeln!(f, "{}", format_response_body(response_body))
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
    pub id: String,
    pub processed_at: String,
    pub attempt: u8,

    // http status code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookLogDetails {
    pub id: String,
    pub processed_at: String,
    pub attempt: u8,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_data: Option<String>,
}

#[derive(Debug, Tabled)]
pub struct TableWebhookLog {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Status", order = 1)]
    pub status: Status,

    #[tabled(order = 2, rename = "Mesage")]
    pub response_message: String,

    #[tabled(order = 3, rename = "HTTP status")]
    pub http_status_code: String,

    #[tabled(order = 4)]
    pub attempt: u8,

    #[tabled(order = 5, rename = "Processed")]
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
        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;

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
                    get_test_webhook_error_message(error_code)
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

impl Display for WebhookLogDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;

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
        } else if let Some(error_code) = &self.error {
            writeln!(
                f,
                "{} {}",
                "Response message:".blue_bold_if_tty(),
                get_test_webhook_error_message(error_code)
            )?;
        } else {
            writeln!(
                f,
                "{} Unknown error",
                "Response message:".blue_bold_if_tty()
            )?;
        }

        if let Some(response_data) = &self.response_data {
            if response_data.is_empty() {
                writeln!(
                    f,
                    "{} {}",
                    "Response data:".blue_bold_if_tty(),
                    format_response_body(response_data)
                )?;
            } else {
                writeln!(f, "{}", "Response data:".blue_bold_if_tty())?;
                writeln!(f, "{}", format_response_body(response_data))?;
            }
        }

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
                get_test_webhook_error_message(error_code).to_string()
            } else {
                "Unknown error".to_string()
            }
        };

        Self {
            id: log.id,
            processed_at: log.processed_at,
            status,
            attempt: log.attempt,
            response_message,
            http_status_code,
        }
    }
}
