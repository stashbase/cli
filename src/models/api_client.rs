use core::fmt;

use log::debug;
use owo_colors::OwoColorize;
use reqwest::{header::HeaderValue, StatusCode};
use serde::Deserialize;

use crate::utils::validation::SECRET_VALUE_MAX_LENGTH;

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
    SearchSecrets {
        project: Option<String>,
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
                    write!(f, "v1/workspace/{}", p)
                }
                None => write!(f, "v1/workspace"),
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
            ApiPath::SearchSecrets { project } => match project {
                Some(p) => write!(f, "v1/projects/{}/secrets-search", p),
                None => write!(f, "v1/secrets-search"),
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
    pub code: ApiErrorEntity,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    fn get_secrets_names_details(self) -> Option<Vec<String>> {
        match self.details {
            Some(d) => {
                let details = serde_json::from_value::<SecretApiErrorDetails>(d);

                match details {
                    Ok(details) => Some(details.secret_names),
                    Err(_) => None,
                }
            }
            None => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretApiErrorDetails {
    pub secret_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogPageNotFoundErrorDetails {
    pub pages_available: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingPermissionErrorDetails {
    // for env/project api keys
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_permissions: Option<Vec<String>>,

    // for personal api key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_workspace_role: Option<PermissionErrorDetails>,

    // for personal api key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_project_role: Option<PermissionErrorDetails>,
}

#[derive(Debug, Deserialize)]
pub struct PermissionErrorDetails {
    pub current: String,
    pub allowed: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpiredApiKeyErrorDetails {
    pub expired_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedApiKeyErrorDetails {
    pub supported_api_key_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TooManyRequestsErrorDetails {
    retry_after: RetryAfterDetails,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetryAfterDetails {
    seconds: usize,

    #[serde(skip)]
    #[allow(dead_code)]
    unix_timestamp: usize,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ApiErrorEntity {
    Generic(GenericError),
    Project(ProjectError),
    Environment(EnvironmentError),
    Secret(SecretsError),
    EnvChangelog(EnvChangelogError),
    Webhook(WebhookError),
}

// TODO: env errors
#[derive(Debug, Deserialize)]
pub enum GenericError {
    #[serde(rename = "server.internal_error")]
    InternalServerError,

    #[serde(rename = "rate_limit.too_many_requests")]
    TooManyRequests,

    #[serde(rename = "auth.unauthorized")]
    Unauthorized,

    #[serde(rename = "auth.expired_api_key")]
    ExpiredApiKey,

    #[serde(rename = "access.unsupported_api_key")]
    UnsupportedApiKey,

    #[serde(rename = "access.missing_permission")]
    MissingPermission,
}

// TODO: env errors
#[derive(Debug, Deserialize)]
pub enum EnvironmentError {
    #[serde(rename = "resource.project_not_found")]
    ProjectNotFound,

    #[serde(rename = "resource.environment_not_found")]
    EnvironmentNotFound,

    #[serde(rename = "resource.compare_to_environment_not_found")]
    CompareToEnvironmentNotFound,

    #[serde(rename = "conflict.environment_already_exists")]
    EnvironmentAlreadyExists,

    #[serde(rename = "conflict.environment_already_unlocked")]
    EnvironmentAlreadyUnlocked,

    #[serde(rename = "conflict.environment_already_locked")]
    EnvironmentAlreadyLocked,

    #[serde(rename = "conflict.current_environment_type")]
    CurrentEnvironmentType,

    #[serde(rename = "resource.environment_locked")]
    EnvironmentLocked,

    #[serde(rename = "quota.environment_limit_reached")]
    EnvironmentLimitReached,

    #[serde(rename = "validation.environment_self_comparison")]
    SelfComparison,

    #[serde(rename = "validation.new_environment_name_equals_original")]
    NewNameEqualsOriginal,
}

#[derive(Debug, Deserialize)]
pub enum ProjectError {
    #[serde(rename = "validation.invalid_project_name")]
    InvalidName,

    #[serde(rename = "conflict.project_already_exists")]
    ProjectAlreadyExists,

    #[serde(rename = "resource.project_not_found")]
    ProjectNotFound,

    #[serde(rename = "access.missing_permission")]
    MissingPermission,

    #[serde(rename = "quota.project_limit_reached")]
    ProjectLimitReached,

    #[serde(rename = "validation.new_project_name_equals_original")]
    NewNameEqualsOriginal,
}

#[derive(Debug, Deserialize)]
pub enum SecretsError {
    #[serde(rename = "resource.secret_not_found")]
    SecretNotFound,

    #[serde(rename = "validation.duplicate_new_names")]
    DuplicateNewNames,

    #[serde(rename = "conflict.secrets_already_exist")]
    SecretsAlreadyExist,

    #[serde(rename = "validation.self_referencing_secrets")]
    SelfReferencingSecrets,

    #[serde(rename = "conflict.self_referencing_secrets")]
    SelfReferencingSecretsConflict,

    #[serde(rename = "validation.secret_description_too_long")]
    SecretDescriptionTooLong,

    #[serde(rename = "validation.secret_values_too_long")]
    SecretValuesTooLong,
}

#[derive(Debug, Deserialize)]
pub enum EnvChangelogError {
    #[serde(rename = "resource.page_not_found")]
    PageNotFound,

    #[serde(rename = "resource.change_not_found")]
    ChangeNotFound,

    #[serde(rename = "conflict.is_current_state")]
    RevertIsCurrentState,
}

#[derive(Debug, Deserialize)]
pub enum WebhookError {
    #[serde(rename = "resource.webhook_not_found")]
    WebhookNotFound,

    #[serde(rename = "conflict.webhook_already_enabled")]
    WebhookAlreadyEnabled,

    #[serde(rename = "conflict.webhook_already_disabled")]
    WebhookAlreadyDisabled,
}

#[derive(Debug)]
pub struct GenericOutputError {
    pub message: String,
    pub hint: Option<String>,
}

#[derive(Debug)]
pub struct SecretsOutputError {
    pub message: String,
    pub hint: Option<String>,
    pub secrets: Option<Vec<String>>,
}

#[derive(Debug)]
pub enum OutputError {
    Generic(GenericOutputError),
    Secrets(SecretsOutputError),
}

impl OutputError {
    pub fn cannot_connect() -> OutputError {
        OutputError::Generic(GenericOutputError {
            message: "could not connect to the API".to_string(),
            hint: Some("please try again later".to_string()),
        })
    }

    pub fn get_message(&self) -> &str {
        match self {
            OutputError::Generic(e) => &e.message,
            OutputError::Secrets(e) => &e.message,
        }
    }

    pub fn get_hint(&self) -> Option<&str> {
        match self {
            OutputError::Generic(e) => e.hint.as_deref(),
            OutputError::Secrets(e) => e.hint.as_deref(),
        }
    }
}

impl From<ApiError> for OutputError {
    fn from(api_error: ApiError) -> Self {
        match &api_error.code {
            ApiErrorEntity::Generic(e) => match e {
                GenericError::InternalServerError => OutputError::Generic(GenericOutputError {
                    message: format!("internal server error"),
                    hint: Some(format!("please try again later")),
                }),
                GenericError::Unauthorized => OutputError::Generic(GenericOutputError {
                    message: format!("you are not authorized"),
                    hint: Some(format!("provide a valid api key")),
                }),
                GenericError::ExpiredApiKey => {
                    let expired_at = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<ExpiredApiKeyErrorDetails>(d);
                        match details {
                            Ok(details) => Some(details.expired_at),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    let message = match expired_at {
                        Some(e) => format!("current api key expired at {}", e),
                        None => format!("current api key is expired"),
                    };

                    OutputError::Generic(GenericOutputError {
                        message,
                        hint: Some(format!("provide new api key and try again")),
                    })
                }
                GenericError::UnsupportedApiKey => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<UnsupportedApiKeyErrorDetails>(d);
                        match details {
                            Ok(details) => Some(format!(
                                "supported api key types for this action: {}",
                                details.supported_api_key_types.join(", ")
                            )),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    OutputError::Generic(GenericOutputError {
                        message: format!("current api key is not supported"),
                        hint,
                    })
                }
                GenericError::MissingPermission => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<MissingPermissionErrorDetails>(d);
                        match details {
                            Ok(details) => {
                                if let Some(permissions) = details.required_permissions {
                                    Some(format!(
                                        "required api key permissions to perform this action: {}",
                                        permissions.join(", ")
                                    ))
                                } else if let Some(r) = details.user_workspace_role {
                                    Some(format!(
                                        "allowed user workspace role to perform this action: {}, current role: {}",
                                        r.allowed.join(", "), r.current
                                    ))
                                } else if let Some(r) = details.user_project_role {
                                    Some(format!(
                                        "allowed project role to perform this action: {}, current role: {}",
                                        r.allowed.join(", "), r.current
                                    ))
                                } else {
                                    None
                                }
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    OutputError::Generic(GenericOutputError {
                        message: format!("missing permission"),
                        hint: hint.or(Some(format!(
                            "current api key does not have permission to perform this action"
                        ))),
                    })
                }
                GenericError::TooManyRequests => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<TooManyRequestsErrorDetails>(d);
                        match details {
                            Ok(d) => {
                                let minutes = (d.retry_after.seconds as f64 / 60.0).ceil() as u32;
                                match minutes == 1 {
                                    true => Some(format!("try again in {} minute", minutes)),
                                    false => Some(format!("try again in {} minutes", minutes)),
                                }
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    OutputError::Generic(GenericOutputError {
                        message: format!("too many requests"),
                        hint,
                    })
                }
            },
            ApiErrorEntity::Secret(e) => match e {
                SecretsError::SecretNotFound => OutputError::Secrets(SecretsOutputError {
                    message: format!("secret not found"),
                    hint: None,
                    secrets: None,
                }),
                SecretsError::DuplicateNewNames => OutputError::Secrets(SecretsOutputError {
                    message: format!("duplicate new names"),
                    hint: Some(format!("cannot change multiple secrets to the same name")),
                    secrets: None,
                }),
                SecretsError::SecretsAlreadyExist => {
                    let secrets = api_error.get_secrets_names_details();
                    OutputError::Secrets(SecretsOutputError {
                        message: format!("cannot rename secrets to already existing secrets"),
                        hint: None,
                        secrets,
                    })
                }
                SecretsError::SelfReferencingSecrets => {
                    let secrets = api_error.get_secrets_names_details();
                    OutputError::Secrets(SecretsOutputError {
                        message: format!("found self-referencing secrets"),
                        hint: None,
                        secrets,
                    })
                }
                SecretsError::SelfReferencingSecretsConflict => {
                    let secrets = api_error.get_secrets_names_details();
                    match secrets {
                        Some(s) if s.len() == 1 => OutputError::Secrets(SecretsOutputError {
                            message: format!("updating this secret would result in self-reference, which is not allowed"),
                            hint: None,
                            secrets: None,
                        }),
                        Some(s) => OutputError::Secrets(SecretsOutputError {
                            message: format!("updating secrets would result in self-reference, which is not allowed"),
                            hint: None,
                            secrets: Some(s),
                        }),
                        None => OutputError::Secrets(SecretsOutputError {
                            message: format!("updating secret would result in self-reference, which is not allowed"),
                            hint: None,
                            secrets: None,
                        }),
                    }
                }
                SecretsError::SecretDescriptionTooLong => {
                    OutputError::Secrets(SecretsOutputError {
                        message: format!("secret description is too long"),
                        hint: api_error.message,
                        secrets: None,
                    })
                }
                SecretsError::SecretValuesTooLong => {
                    let secrets = api_error.get_secrets_names_details();
                    OutputError::Secrets(SecretsOutputError {
                        message: format!(
                            "secret values are too long (max {} characters)",
                            SECRET_VALUE_MAX_LENGTH
                        ),
                        hint: None,
                        secrets,
                    })
                }
            },
            // For other error types, convert them to Generic errors
            _ => OutputError::Generic(GenericOutputError {
                message: format!("unexpected error"),
                hint: None,
            }),
        }
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // writeln!(
        //     f,
        //     "{}",
        //     "Error".if_supports_color(Stream::Stderr, |text| text.red())
        // )?;
        writeln!(f, "{}", "Error".red().bold())?;

        let message = self.get_message();
        let hint = self.get_hint();

        writeln!(f, "- message: {}", message)?;

        if let Some(hint) = hint {
            write!(f, "- hint: {}", hint)?;
        }

        if let OutputError::Secrets(e) = self {
            if let Some(secrets) = e.secrets.as_ref() {
                writeln!(f, "- secrets: {}", secrets.join(", "))?;
            }
        }

        Ok(())
    }
}
