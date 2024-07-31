use core::fmt;

use owo_colors::OwoColorize;
use reqwest::{header::HeaderValue, StatusCode};
use serde::Deserialize;

#[derive(Debug)]
pub struct RequestArgs {
    pub api_key: String,
    pub path: ApiPath,
    pub query: Option<Vec<(String, String)>>,
}

#[derive(Debug)]
pub enum ApiPath {
    Projects(Option<String>),
    Environments {
        project: String,
        path: Option<String>,
    },

    EnvChangelog {
        project: String,
        environment: String,
        path: Option<String>,
    },

    Webhooks {
        project: String,
        environment: String,
        path: Option<String>,
    },
    Secrets {
        project: String,
        environment: String,
        path: Option<String>,
    },
    Workspace {
        path: Option<String>,
    },
}

impl fmt::Display for ApiPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiPath::Projects(p) => match p {
                Some(value) => write!(f, "v1/projects/{}", value),
                None => write!(f, "v1/projects"),
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
            ApiPath::EnvChangelog {
                project,
                environment,
                path,
            } => match path {
                Some(p) => {
                    write!(
                        f,
                        "v1/projects/{}/environments/{}/changelog/{}",
                        project, environment, p
                    )
                }
                None => {
                    write!(
                        f,
                        "v1/projects/{}/environments/{}/changelog",
                        project, environment
                    )
                }
            },
            ApiPath::Workspace { path } => match path {
                Some(p) => {
                    write!(f, "workspace/{}", p)
                }
                None => write!(f, "workspace"),
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
        }
    }
}

// NOTE: GET
#[derive(Debug)]
pub struct GetApiResponseOk {
    pub status: StatusCode,
    pub text: String,
}

#[derive(Debug)]
pub enum GetRequestApiResponse {
    Ok(GetApiResponseOk),
    Err(CustomError),
}

// NOTE: POST, PATCH, PUT
#[derive(Debug)]
pub enum RequestApiOptionResponse {
    Ok(OptionResponseOk),
    Err(CustomError),
}

#[derive(Debug)]
pub struct OptionResponseOk {
    pub status: StatusCode,
    pub text: Option<String>,
}

// NOTE: DELETE
#[derive(Debug)]
pub enum DeleteRequestApiResponse {
    Ok(DeleteApiResponseOk),
    Err(CustomError),
}

pub type DeleteApiResponseOk = OptionResponseOk;

// error

#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub code: ApiErrorEntity,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretApiErrorDetails {
    pub secret_keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ApiErrorEntity {
    Project(ProjectError),
    Environment(EnvironmentError),
    Secret(SecretsError),
    EnvChangelog(EnvChangelogError),
    Webhook(WebhookError),
}

// TODO: env errors
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentError {
    ProjectNotFound,
    EnvironmentNotFound,
    CompareToEnvironmentNotFound,
    EnvironmentAlreadyExists,
    EnvironmentAlreadyLocked,
    EnvironmentAlreadyUnlocked,
    CurrentEnvironmentType,
    EnvironmentLocked,
    #[serde(rename = "environment_limit_reached")]
    LimitReached,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectError {
    InvalidName,
    ProjectAlreadyExists,
    ProjectNotFound,
    MissingPermission,
    #[serde(rename = "project_limit_reached")]
    LimitReached,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretsError {
    SecretNotFound,
    DuplicateNewKeys,
    ExistingDuplicates,
    SelfReferencingSecrets,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvChangelogError {
    PageNotFound,
    ChangeNotFound,
    RenameEnvironmentAlreadyExists,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookError {
    WebhookNotFound,
    WebhookAlreadyEnabled,
    WebhookAlreadyDisabled,
}

#[derive(Debug)]
pub struct CustomError {
    pub message: String,
    pub hint: Option<String>,
}

impl CustomError {
    pub fn rate_limit_reached(reset_header: Option<&HeaderValue>) -> CustomError {
        match reset_header {
            Some(header) => {
                let seconds = header.to_str().unwrap_or("0").parse::<u64>().unwrap_or(0);
                let minutes = (seconds as f64 / 60.0).ceil() as u32;

                Self {
                    message: "Too many requests".to_string(),
                    hint: match minutes == 1 {
                        false => Some(format!("Try again in {} minutes", minutes)),
                        true => Some(format!("Try again in {} minute", minutes)),
                    },
                }
            }
            None => Self {
                message: "Too many requests".to_string(),
                hint: Some("Try again later".to_string()),
            },
        }
    }

    pub fn unauthorized() -> CustomError {
        Self {
            message: "User unauthorized".to_string(),
            hint: Some("Check your API key".to_string()),
        }
    }

    pub fn unknown() -> CustomError {
        Self {
            message: "Unknown error".to_string(),
            hint: Some("Try again later".to_string()),
        }
    }
}

impl From<ApiError> for CustomError {
    fn from(api_error: ApiError) -> Self {
        match api_error.code {
            ApiErrorEntity::Project(e) => match e {
                ProjectError::InvalidName => CustomError {
                    message: format!("Invalid name"),
                    hint: None,
                },

                ProjectError::ProjectNotFound => CustomError {
                    message: format!("project not found"),
                    hint: None,
                },

                ProjectError::LimitReached => CustomError {
                    message: format!("project limit reached"),
                    hint: Some(format!(
                        "workspace reached the maximum number of projects allowed"
                    )),
                },
                ProjectError::ProjectAlreadyExists => CustomError {
                    message: format!("project already exists"),
                    hint: Some(format!("use a different name")),
                },
                ProjectError::MissingPermission => CustomError {
                    message: format!("missing permission"),
                    hint: Some(format!("you do not have permission to perform this action")),
                },
            },
            ApiErrorEntity::Environment(e) => match e {
                EnvironmentError::LimitReached => CustomError {
                    message: format!("environment limit reached"),
                    hint: Some(format!(
                        "project reached the maximum number of environments allowed"
                    )),
                },
                EnvironmentError::ProjectNotFound => CustomError {
                    message: format!("project not found"),
                    hint: None,
                },
                EnvironmentError::EnvironmentNotFound => CustomError {
                    message: format!("environment not found"),
                    hint: None,
                },

                EnvironmentError::CompareToEnvironmentNotFound => CustomError {
                    message: format!("environment not found (second environment)"),
                    hint: None,
                },
                EnvironmentError::EnvironmentAlreadyExists => CustomError {
                    message: format!("environment already exists"),
                    hint: Some(format!("use a different name")),
                },
                EnvironmentError::EnvironmentAlreadyLocked => CustomError {
                    message: format!("environment already locked"),
                    hint: None,
                },
                EnvironmentError::EnvironmentAlreadyUnlocked => CustomError {
                    message: format!("environment already unlocked"),
                    hint: None,
                },
                EnvironmentError::CurrentEnvironmentType => CustomError {
                    message: format!("current environment type"),
                    hint: Some(format!("cannot update to same type")),
                },
                EnvironmentError::EnvironmentLocked => CustomError {
                    message: format!("this environment is locked"),
                    hint: Some(format!("unlock environment to perform this action")),
                },
            },
            ApiErrorEntity::Secret(e) => match e {
                SecretsError::SecretNotFound => CustomError {
                    message: format!("secret not found"),
                    hint: None,
                },

                SecretsError::DuplicateNewKeys => CustomError {
                    message: format!("duplicate new keys"),
                    hint: Some(format!("cannot change multiple secrets to the same key")),
                },

                SecretsError::ExistingDuplicates => CustomError {
                    message: format!("cannot renamed to already existing keys"),
                    hint: Some(format!(
                        "secrets already exists: {}",
                        api_error.details.unwrap()
                    )),
                },

                SecretsError::SelfReferencingSecrets => {
                    let secrets = match api_error.details {
                        Some(d) => {
                            let details = serde_json::from_value::<SecretApiErrorDetails>(d);

                            match details {
                                Ok(details) => Some(details.secret_keys.join(", ")),
                                Err(_) => None,
                            }
                        }
                        None => None,
                    };

                    return CustomError {
                        message: format!("found self-referencing secrets"),
                        hint: secrets,
                    };
                }
            },
            ApiErrorEntity::EnvChangelog(e) => match e {
                EnvChangelogError::PageNotFound => CustomError {
                    message: format!("page not found"),
                    hint: match api_error.message {
                        Some(m) => Some(m.to_lowercase()),
                        None => None,
                    },
                },
                EnvChangelogError::ChangeNotFound => CustomError {
                    message: format!("change record not found"),
                    hint: Some(format!("make sure that the id is correct")),
                },

                EnvChangelogError::RenameEnvironmentAlreadyExists => CustomError {
                    message: format!("cannot revert environment rename"),
                    hint: Some(format!("environment with the name already exists")),
                },
            },
            ApiErrorEntity::Webhook(e) => match e {
                WebhookError::WebhookNotFound => CustomError {
                    message: format!("webhook not found"),
                    hint: None,
                },
                WebhookError::WebhookAlreadyEnabled => CustomError {
                    message: format!("webhook already enabled"),
                    hint: None,
                },
                WebhookError::WebhookAlreadyDisabled => CustomError {
                    message: format!("webhook already disabled"),
                    hint: None,
                },
            },
        }
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // writeln!(
        //     f,
        //     "{}",
        //     "Error".if_supports_color(Stream::Stderr, |text| text.red())
        // )?;
        writeln!(f, "{}", "Error".red().bold())?;

        if let Some(hint) = &self.hint {
            writeln!(f, "- message: {}", self.message)?;
            write!(f, "- hint: {}", hint)?;
        } else {
            write!(f, "- message: {}", self.message)?;
        }

        Ok(())
    }
}
