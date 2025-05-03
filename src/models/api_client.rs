use core::fmt;

use colored_json::to_colored_json_auto;
use log::debug;
use owo_colors::OwoColorize;
use reqwest::{header::HeaderValue, StatusCode};
use serde::{Deserialize, Serialize};

use crate::utils::{
    output::{get_colored_json, write_indented},
    validation::SECRET_VALUE_MAX_LENGTH,
};

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
    Whoami,
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
            ApiPath::Whoami => write!(f, "v1/whoami"),
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
pub struct MissingPermissionErrorDetails {
    // for env/project api keys
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_permissions: Option<Vec<String>>,

    // for personal api key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_workspace_role: Option<PermissionErrorDetails>,

    // for personal api key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_environment_role: Option<PermissionErrorDetails>,
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

    #[serde(rename = "access.ip_address_not_allowed")]
    IpAddressNotAllowed,

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

    #[serde(rename = "access.missing_full_project_access")]
    MissingFullProjectAccess,

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

    #[serde(rename = "validation.secret_comment_too_long")]
    SecretCommentTooLong,

    #[serde(rename = "validation.secret_values_too_long")]
    SecretValuesTooLong,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct GenericOutputError {
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>, // error code from API response

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretsOutputError {
    pub code: String, // error code from API response
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<String>>,
}

#[derive(Debug)]
pub enum OutputError {
    Generic(GenericOutputError),
    Secrets(SecretsOutputError),
}

impl serde::Serialize for OutputError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            OutputError::Generic(e) => e.serialize(serializer),
            OutputError::Secrets(e) => e.serialize(serializer),
        }
    }
}

impl OutputError {
    pub fn cannot_connect() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Could not connect to the API.".to_string(),
            hint: Some("Please try again later.".to_string()),
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

    pub fn get_code(&self) -> Option<&str> {
        match self {
            OutputError::Generic(e) => e.code.as_deref(),
            OutputError::Secrets(e) => Some(&e.code),
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        // Create a wrapper struct inline for serialization
        #[derive(serde::Serialize)]
        struct ErrorWrapper<'a> {
            error: &'a OutputError,
        }

        let wrapper = ErrorWrapper { error: self };
        serde_json::to_value(&wrapper)
    }

    pub fn to_colored_json(&self) -> Result<String, serde_json::Error> {
        let json_value = self.to_json_value()?;
        let json_str = to_colored_json_auto(&json_value)?;

        Ok(json_str)
    }
}

impl From<ApiError> for OutputError {
    fn from(api_error: ApiError) -> Self {
        match &api_error.code {
            ApiErrorEntity::Generic(e) => match e {
                GenericError::InternalServerError => OutputError::Generic(GenericOutputError {
                    code: Some("server.internal_error".to_string()),
                    message: format!("Internal server error."),
                    hint: Some(format!("Please try again later.")),
                }),
                GenericError::Unauthorized => OutputError::Generic(GenericOutputError {
                    code: Some("auth.unauthorized".to_string()),
                    message: format!("You are not authorized."),
                    hint: Some(format!("Provide a valid api key.")),
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
                        Some(e) => format!("Current api key expired at {}.", e),
                        None => format!("Current api key is expired."),
                    };

                    OutputError::Generic(GenericOutputError {
                        message,
                        code: Some("auth.expired_api_key".to_string()),
                        hint: Some(format!("Provide a new api key and try again.")),
                    })
                }
                GenericError::UnsupportedApiKey => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<UnsupportedApiKeyErrorDetails>(d);
                        match details {
                            Ok(details) => Some(format!(
                                "Supported api key types for this action: {}.",
                                details.supported_api_key_types.join(", ")
                            )),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    OutputError::Generic(GenericOutputError {
                        message: format!("Current api key is not supported."),
                        code: Some("access.unsupported_api_key".to_string()),
                        hint,
                    })
                }
                GenericError::IpAddressNotAllowed => OutputError::Generic(GenericOutputError {
                    message: format!("IP address not allowed."),
                    code: Some("access.ip_address_not_allowed".to_string()),
                    hint: Some(format!(
                        "Access denied, the IP of the request is not allowed to access the API."
                    )),
                }),
                GenericError::MissingPermission => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<MissingPermissionErrorDetails>(d);
                        match details {
                            Ok(details) => {
                                if let Some(permissions) = details.required_permissions {
                                    Some(format!(
                                        "Required api key permissions to perform this action: {}.",
                                        permissions.join(", ")
                                    ))
                                } else if let Some(r) = details.user_workspace_role {
                                    Some(format!(
                                        "Allowed user workspace role to perform this action: {}, current role: {}.",
                                        r.allowed.join(", "), r.current
                                    ))
                                } else if let Some(r) = details.user_environment_role {
                                    Some(format!(
                                        "Allowed environment role to perform this action: {}, current role: {}.",
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
                        message: format!("Missing permission."),
                        code: Some("access.missing_permission".to_string()),
                        hint: hint.or(Some(format!(
                            "Current api key does not have permission to perform this action."
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
                        message: format!("Too many requests."),
                        code: Some("rate_limit.too_many_requests".to_string()),
                        hint,
                    })
                }
            },

            ApiErrorEntity::Project(e) => match e {
                ProjectError::InvalidName => OutputError::Generic(GenericOutputError {
                    message: format!("Invalid project name."),
                    code: Some(format!("validation.invalid_project_name")),
                    hint: None,
                }),

                ProjectError::ProjectNotFound => OutputError::Generic(GenericOutputError {
                    message: format!("Project not found."),
                    code: Some(format!("resource.project_not_found")),
                    hint: None,
                }),

                ProjectError::ProjectLimitReached => OutputError::Generic(GenericOutputError {
                    message: format!("Project limit reached."),
                    code: Some(format!("quota.project_limit_reached")),
                    hint: Some(format!(
                        "Workspace reached the maximum number of projects allowed."
                    )),
                }),

                ProjectError::ProjectAlreadyExists => OutputError::Generic(GenericOutputError {
                    message: format!("Project already exists."),
                    code: Some(format!("conflict.project_already_exists")),
                    hint: Some(format!("Use a different name.")),
                }),

                ProjectError::MissingPermission => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<MissingPermissionErrorDetails>(d);

                        match details {
                            Ok(details) => {
                                if let Some(permissions) = details.required_permissions {
                                    let msg = format!(
                                        "Required api key permissions to perform this action: {}.",
                                        permissions.join(", ")
                                    );
                                    Some(msg)
                                } else if let Some(r) = details.user_workspace_role {
                                    let msg = format!(
                                        "Allowed user workspace role to perform this action: {}, current role: {}.",
                                        r.allowed.join(", "), r.current
                                    );
                                    Some(msg)
                                } else if let Some(r) = details.user_environment_role {
                                    let msg = format!(
                                        "Allowed environment role to perform this action: {}, current role: {}.",
                                        r.allowed.join(", "), r.current
                                    );
                                    Some(msg)
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
                        message: format!("Missing permission."),
                        code: Some(format!("access.missing_permission")),
                        hint: match hint {
                            Some(h) => Some(h),
                            None => {
                                Some(format!("You do not have permission to perform this action."))
                            }
                        },
                    })
                }
                ProjectError::MissingFullProjectAccess => OutputError::Generic(GenericOutputError {
                    message: format!("Missing full project access."),
                    code: Some(format!("access.missing_full_project_access")),
                    hint: Some(format!("Full project access is required to perform this action (project creator or workspace admin/owner).")),
                }),

                ProjectError::NewNameEqualsOriginal => OutputError::Generic(GenericOutputError {
                    message: format!("New project name equals original."),
                    code: Some(format!("validation.new_project_name_equals_original")),
                    hint: Some(format!("Use a different new name.")),
                }),
            },
            ApiErrorEntity::Environment(e) => match e {
                EnvironmentError::EnvironmentLimitReached => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("Environment limit reached."),
                        code: Some(format!("quota.environment_limit_reached")),
                        hint: Some(format!(
                            "Project reached the maximum number of environments allowed."
                        )),
                    })
                }
                EnvironmentError::ProjectNotFound => OutputError::Generic(GenericOutputError {
                    message: format!("Project not found."),
                    code: Some(format!("resource.project_not_found")),
                    hint: None,
                }),
                EnvironmentError::EnvironmentNotFound => OutputError::Generic(GenericOutputError {
                    message: format!("Environment not found."),
                    code: Some(format!("resource.environment_not_found")),
                    hint: None,
                }),
                EnvironmentError::CompareToEnvironmentNotFound => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("Environment not found (second environment)."),
                        code: Some(format!("resource.compare_to_environment_not_found")),
                        hint: None,
                    })
                }
                EnvironmentError::EnvironmentAlreadyExists => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("Environment already exists."),
                        code: Some(format!("conflict.environment_already_exists")),
                        hint: Some(format!("Use a different name.")),
                    })
                }
                EnvironmentError::EnvironmentAlreadyLocked => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("Environment already locked."),
                        code: Some(format!("conflict.environment_already_locked")),
                        hint: None,
                    })
                }
                EnvironmentError::EnvironmentAlreadyUnlocked => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("Environment already unlocked."),
                        code: Some(format!("conflict.environment_already_unlocked")),
                        hint: None,
                    })
                }
                EnvironmentError::CurrentEnvironmentType => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("Current environment type."),
                        code: Some(format!("conflict.current_environment_type")),
                        hint: Some(format!("Cannot update to same type.")),
                    })
                }
                EnvironmentError::EnvironmentLocked => OutputError::Generic(GenericOutputError {
                    message: format!("This environment is locked."),
                    code: Some(format!("resource.environment_locked")),
                    hint: Some(format!("Unlock environment to perform this action.")),
                }),
                EnvironmentError::SelfComparison => OutputError::Generic(GenericOutputError {
                    message: "Environment comparing with itself".to_string(),
                    code: Some(format!("validation.environment_self_comparison")),
                    hint: Some(format!("Use different environment for comparison.")),
                }),
                EnvironmentError::NewNameEqualsOriginal => {
                    OutputError::Generic(GenericOutputError {
                        message: format!("New environment name equals original."),
                        code: Some(format!("validation.new_environment_name_equals_original")),
                        hint: Some(format!("Use a different new name.")),
                    })
                }
            },

            ApiErrorEntity::Webhook(e) => match e {
                WebhookError::WebhookNotFound => OutputError::Generic(GenericOutputError {
                    message: format!("Webhook not found."),
                    code: Some(format!("resource.webhook_not_found")),
                    hint: None,
                }),
                WebhookError::WebhookAlreadyEnabled => OutputError::Generic(GenericOutputError {
                    message: format!("Webhook already enabled."),
                    code: Some(format!("conflict.webhook_already_enabled")),
                    hint: None,
                }),
                WebhookError::WebhookAlreadyDisabled => OutputError::Generic(GenericOutputError {
                    message: format!("Webhook already disabled."),
                    code: Some(format!("conflict.webhook_already_disabled")),
                    hint: None,
                }),
            },
            ApiErrorEntity::Secret(e) => match e {
                SecretsError::SecretNotFound => OutputError::Secrets(SecretsOutputError {
                    message: format!("Secret not found."),
                    code: format!("resource.secret_not_found"),
                    hint: None,
                    secrets: None,
                }),
                SecretsError::DuplicateNewNames => OutputError::Secrets(SecretsOutputError {
                    message: format!("Duplicate new names."),
                    code: format!("validation.duplicate_new_names"),
                    hint: Some(format!("Cannot change multiple secrets to the same name.")),
                    secrets: None,
                }),
                SecretsError::SecretsAlreadyExist => {
                    let secrets = api_error.get_secrets_names_details();
                    OutputError::Secrets(SecretsOutputError {
                        message: format!("Cannot rename secrets to already existing secrets."),
                        code: format!("conflict.secrets_already_exist"),
                        hint: None,
                        secrets,
                    })
                }
                SecretsError::SelfReferencingSecrets => {
                    let secrets = api_error.get_secrets_names_details();
                    OutputError::Secrets(SecretsOutputError {
                        message: format!("Found self-referencing secrets."),
                        code: format!("validation.self_referencing_secrets"),
                        hint: None,
                        secrets,
                    })
                }
                SecretsError::SelfReferencingSecretsConflict => {
                    let secrets = api_error.get_secrets_names_details();
                    match secrets {
                        Some(s) if s.len() == 1 => OutputError::Secrets(SecretsOutputError {
                            message: format!("Updating this secret would result in self-reference, which is not allowed."),
                            code: format!("validation.self_referencing_secrets"),
                            hint: None,
                            secrets: None,
                        }),
                        Some(s) => OutputError::Secrets(SecretsOutputError {
                            message: format!("Updating secrets would result in self-reference, which is not allowed."),
                            code: format!("validation.self_referencing_secrets"),
                            hint: None,
                            secrets: Some(s),
                        }),
                        None => OutputError::Secrets(SecretsOutputError {
                            message: format!("Updating secret would result in self-reference, which is not allowed."),
                            code: format!("validation.self_referencing_secrets"),
                            hint: None,
                            secrets: None,
                        }),
                    }
                }
                SecretsError::SecretCommentTooLong => OutputError::Secrets(SecretsOutputError {
                    code: format!("validation.secret_comment_too_long"),
                    message: format!("Secret comment is too long."),
                    hint: api_error.message,
                    secrets: None,
                }),
                SecretsError::SecretValuesTooLong => {
                    let secrets = api_error.get_secrets_names_details();
                    OutputError::Secrets(SecretsOutputError {
                        code: format!("validation.secret_values_too_long"),
                        message: format!(
                            "Secret values are too long (max {} characters).",
                            SECRET_VALUE_MAX_LENGTH
                        ),
                        hint: None,
                        secrets,
                    })
                }
            },
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
        writeln!(f, "{}", "API Error".red().bold())?;

        let message = self.get_message();
        let hint = self.get_hint();
        let code = self.get_code();

        if let Some(code) = code {
            writeln!(f, "  Code: {}", code)?;
        }

        write!(f, "  Message: {}", message)?;

        if let Some(hint) = hint {
            write!(f, "\n  Hint: {}", hint)?;
        }

        if let OutputError::Secrets(e) = self {
            if let Some(secrets) = e.secrets.as_ref() {
                write!(f, "\n  Secrets: {}", secrets.join(", "))?;
            }
        }

        Ok(())
    }
}
