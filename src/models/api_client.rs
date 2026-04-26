use core::fmt;

use colored_json::to_colored_json_auto;
use owo_colors::OwoColorize;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::utils::output::is_color_enabled;

#[derive(Debug)]
pub struct RequestArgs {
    pub api_key: String,
    pub path: ApiPath,
    pub query: Option<Vec<(String, String)>>,
}

#[derive(Debug)]
pub enum ApiPath {
    Projects(Option<String>),
    EnvironmentEnvScope {
        path: Option<String>,
    },
    Environments {
        project: String,
        path: Option<String>,
    },
    Webhooks {
        project: String,
        environment: String,
        path: Option<String>,
    },
    WebhooksEnvScope {
        path: Option<String>,
    },
    Secrets {
        project: String,
        environment: String,
        path: Option<String>,
    },
    SecretsEnvScope {
        path: Option<String>,
    },
    SearchSecrets {
        project: String,
    },
    Scan {
        path: String,
    },
    Workspace {
        path: Option<String>,
    },
    Whoami,
}

impl fmt::Display for ApiPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiPath::Projects(p) => match p {
                Some(value) => write!(f, "v1/projects/{}", value),
                None => write!(f, "v1/projects"),
            },
            ApiPath::EnvironmentEnvScope { path } => match path {
                Some(value) => write!(f, "v1/environment/{}", value),
                None => write!(f, "v1/environment"),
            },
            ApiPath::Environments { project, path } => match path {
                Some(value) => write!(f, "v1/projects/{}/environments/{}", project, value),
                None => write!(f, "v1/projects/{}/environments", project),
            },
            ApiPath::Secrets {
                project,
                environment,
                path,
            } => match path {
                Some(p) => write!(
                    f,
                    "v1/projects/{}/environments/{}/secrets/{}",
                    project, environment, p
                ),
                None => write!(
                    f,
                    "v1/projects/{}/environments/{}/secrets",
                    project, environment,
                ),
            },
            ApiPath::SecretsEnvScope { path } => match path {
                Some(p) => write!(f, "v1/environment/secrets/{}", p),
                None => write!(f, "v1/environment/secrets"),
            },
            ApiPath::Scan { path } => write!(f, "v1/scan/{}", path),
            ApiPath::Workspace { path } => match path {
                Some(p) => {
                    write!(f, "v1/workspace/{}", p)
                }
                None => write!(f, "v1/workspace"),
            },

            ApiPath::WebhooksEnvScope { path } => match path {
                Some(p) => write!(f, "v1/environment/webhooks/{}", p),
                None => write!(f, "v1/environment/webhooks"),
            },
            ApiPath::Webhooks {
                project,
                environment,
                path,
            } => match path {
                Some(p) => {
                    write!(
                        f,
                        "v1/projects/{}/environments/{}/webhooks/{}",
                        project, environment, p
                    )
                }
                None => {
                    write!(
                        f,
                        "v1/projects/{}/environments/{}/webhooks",
                        project, environment
                    )
                }
            },
            ApiPath::SearchSecrets { project } => {
                write!(f, "v1/projects/{}/secrets/search", project)
            }
            ApiPath::Whoami => write!(f, "v1/whoami"),
        }
    }
}

// NOTE: GET
#[derive(Debug)]
pub struct GetApiResponseOk {
    #[allow(dead_code)]
    pub status: StatusCode,
    pub text: String,
}

#[derive(Debug)]
pub enum GetRequestApiResponse {
    Ok(GetApiResponseOk),
    Err(OutputError),
}

// NOTE: POST, PATCH, PUT
#[derive(Debug)]
pub enum RequestApiOptionResponse {
    Ok(OptionResponseOk),
    Err(OutputError),
}

#[derive(Debug)]
pub struct OptionResponseOk {
    #[allow(dead_code)]
    pub status: StatusCode,
    pub text: Option<String>,
}

// NOTE: DELETE
#[derive(Debug)]
pub enum DeleteRequestApiResponse {
    Ok(DeleteApiResponseOk),
    Err(OutputError),
}

pub type DeleteApiResponseOk = OptionResponseOk;

// error

#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub code: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[allow(dead_code)]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenericOutputError {
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>, // error code from API response

    #[serde(skip_serializing)]
    pub status: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug)]
pub enum OutputError {
    Generic(GenericOutputError),
}

impl serde::Serialize for OutputError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            OutputError::Generic(e) => e.serialize(serializer),
        }
    }
}

impl OutputError {
    pub fn failed_to_read_response_body() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Failed to read response body.".to_string(),
            status: None,
            hint: Some("Please try again later.".to_string()),
            details: None,
        })
    }

    pub fn failed_to_deserialize_response_body() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Failed to deserialize response body.".to_string(),
            status: None,
            hint: Some("Please try again later.".to_string()),
            details: None,
        })
    }

    pub fn cannot_connect() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Could not connect to the API.".to_string(),
            status: None,
            hint: Some("Please try again later.".to_string()),
            details: None,
        })
    }

    pub fn request_timed_out() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: Some("request.timeout".to_string()),
            message: "Request timed out.".to_string(),
            status: None,
            hint: Some("Increase timeout with --timeout and try again.".to_string()),
            details: None,
        })
    }

    pub fn request_aborted() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: Some("request.aborted".to_string()),
            message: "Request canceled by user.".to_string(),
            status: None,
            hint: None,
            details: None,
        })
    }

    pub fn get_message(&self) -> &str {
        match self {
            OutputError::Generic(e) => &e.message,
        }
    }

    pub fn get_hint(&self) -> Option<&str> {
        match self {
            OutputError::Generic(e) => e.hint.as_deref(),
        }
    }

    pub fn get_code(&self) -> Option<&str> {
        match self {
            OutputError::Generic(e) => e.code.as_deref(),
        }
    }

    pub fn get_details(&self) -> Option<&serde_json::Value> {
        match self {
            OutputError::Generic(e) => e.details.as_ref(),
        }
    }

    pub fn get_status(&self) -> Option<u16> {
        match self {
            OutputError::Generic(e) => e.status,
        }
    }

    pub fn with_status(mut self, status: Option<u16>) -> OutputError {
        match &mut self {
            OutputError::Generic(e) => e.status = status,
        }

        self
    }

    pub fn format_error_output(self, json_format: bool) -> Result<String, serde_json::Error> {
        if json_format {
            let json_err = self.to_formatted_json_string()?;
            Ok(json_err)
        } else {
            Ok(self.to_string())
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        let mut error = serde_json::Map::new();

        if let Some(code) = self.get_code() {
            error.insert("code".to_string(), serde_json::json!(code));
        }

        error.insert("message".to_string(), serde_json::json!(self.get_message()));

        if let Some(hint) = self.get_hint() {
            error.insert("hint".to_string(), serde_json::json!(hint));
        }

        if let Some(details) = self.get_details() {
            error.insert("details".to_string(), details.clone());
        }

        let mut root = serde_json::Map::new();
        root.insert("ok".to_string(), serde_json::json!(false));
        if let Some(status) = self.get_status() {
            root.insert("status".to_string(), serde_json::json!(status));
        }
        root.insert("error".to_string(), serde_json::Value::Object(error));

        Ok(serde_json::Value::Object(root))
    }

    pub fn to_formatted_json_string(&self) -> Result<String, serde_json::Error> {
        let json_value = self.to_json_value()?;

        let json_str = if is_color_enabled(false) {
            to_colored_json_auto(&json_value)?
        } else {
            serde_json::to_string_pretty(&json_value)?
        };

        Ok(json_str)
    }
}

impl From<ApiError> for OutputError {
    fn from(e: ApiError) -> Self {
        OutputError::Generic(GenericOutputError {
            code: Some(e.code),
            message: e.message.unwrap_or_else(|| "Unknown error".to_string()),
            status: None,
            hint: e.hint,
            details: e.details,
        })
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if is_color_enabled(false) {
            if let Some(status) = self.get_status() {
                writeln!(f, "{}", format!("API Error ({})", status).red().bold())?;
            } else {
                writeln!(f, "{}", "API Error".red().bold())?;
            }
        } else {
            if let Some(status) = self.get_status() {
                writeln!(f, "API Error ({})", status)?;
            } else {
                writeln!(f, "{}", "API Error")?;
            }
        }

        let message = self.get_message();
        let hint = self.get_hint();
        let code = self.get_code();
        let details = self.get_details();

        if let Some(code) = code {
            writeln!(f, "  Code: {}", code)?;
        }

        write!(f, "  Message: {}", message)?;

        if let Some(hint) = hint {
            write!(f, "\n  Hint: {}", hint)?;
        }

        if let Some(details) = details {
            write!(f, "\n  Details:")?;
            write_pretty_details(f, details, 4)?;
        }

        Ok(())
    }
}

fn write_pretty_details(
    f: &mut fmt::Formatter<'_>,
    value: &serde_json::Value,
    indent: usize,
) -> fmt::Result {
    let pad = " ".repeat(indent);

    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                write!(f, "\n{}(empty)", pad)?;
                return Ok(());
            }

            for (key, val) in map {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        write!(f, "\n{}{}:", pad, key)?;
                        write_pretty_details(f, val, indent + 2)?;
                    }
                    _ => {
                        write!(f, "\n{}{}: {}", pad, key, json_scalar_to_string(val))?;
                    }
                }
            }
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                write!(f, "\n{}[]", pad)?;
                return Ok(());
            }

            for item in items {
                match item {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        write!(f, "\n{}- ", pad)?;
                        write_pretty_details(f, item, indent + 2)?;
                    }
                    _ => {
                        write!(f, "\n{}- {}", pad, json_scalar_to_string(item))?;
                    }
                }
            }
        }
        _ => {
            write!(f, "\n{}{}", pad, json_scalar_to_string(value))?;
        }
    }

    Ok(())
}

fn json_scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
