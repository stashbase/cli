use core::fmt;

use colored_json::to_colored_json_auto;
use owo_colors::OwoColorize;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::utils::{output::is_color_enabled, validation::SECRET_VALUE_MAX_LENGTH};

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
pub struct SecretApiErrorDetails {
    pub secret_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
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
pub struct ExpiredApiKeyErrorDetails {
    pub expired_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UnsupportedApiKeyErrorDetails {
    pub supported_api_key_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TooManyRequestsErrorDetails {
    retry_after: RetryAfterDetails,
}

#[derive(Debug, Deserialize)]
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
    Scan(ScanError),
    Webhook(WebhookError),
}

#[derive(Debug, Deserialize)]
pub enum GenericError {
    #[serde(rename = "server.temporary_unavailable")]
    ServerTemporaryUnavailable,

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

    #[serde(rename = "access.unsupported_api_key_type")]
    UnsupportedApiKeyType,

    #[serde(rename = "access.missing_permission")]
    MissingPermission,
}

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

    #[serde(rename = "quota.webhook_limit_reached")]
    WebhookLimitReached,
}

#[derive(Debug, Deserialize)]
pub enum ScanError {
    #[serde(rename = "rate_limit.scan_rpm_limit_reached")]
    ScanRpmLimitReached,

    #[serde(rename = "rate_limit.scan_request_too_large")]
    ScanRequestTooLarge,

    #[serde(
        rename = "validation.invalid_ignored_secret_regex",
        alias = "validation.invalid_ignore_value_regex"
    )]
    InvalidIgnoredSecretRegex,

    #[serde(
        rename = "validation.invalid_ignored_secret_hash",
        alias = "validation.invalid_ignore_value_hash"
    )]
    InvalidIgnoredSecretHash,
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
    pub fn failed_to_read_response_body() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Failed to read response body.".to_string(),
            hint: Some("Please try again later.".to_string()),
        })
    }

    pub fn failed_to_deserialize_response_body() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Failed to deserialize response body.".to_string(),
            hint: Some("Please try again later.".to_string()),
        })
    }

    pub fn cannot_connect() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: None,
            message: "Could not connect to the API.".to_string(),
            hint: Some("Please try again later.".to_string()),
        })
    }

    pub fn request_timed_out() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: Some("request.timeout".to_string()),
            message: "Request timed out.".to_string(),
            hint: Some("Increase timeout with --timeout and try again.".to_string()),
        })
    }

    pub fn request_aborted() -> OutputError {
        OutputError::Generic(GenericOutputError {
            code: Some("request.aborted".to_string()),
            message: "Request canceled by user.".to_string(),
            hint: None,
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

    pub fn format_error_output(self, json_format: bool) -> Result<String, serde_json::Error> {
        if json_format {
            let json_err = self.to_formatted_json_string()?;
            Ok(json_err)
        } else {
            Ok(self.to_string())
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        #[derive(serde::Serialize)]
        struct ErrorWrapper<'a> {
            #[serde(rename = "error")]
            error: ErrorData<'a>,
        }

        #[derive(serde::Serialize)]
        struct ErrorData<'a> {
            #[serde(flatten)]
            data: &'a OutputError,
            #[serde(rename = "type")]
            error_type: &'static str,
        }

        let wrapper = ErrorWrapper {
            error: ErrorData {
                data: self,
                error_type: "api_error",
            },
        };

        serde_json::to_value(&wrapper)
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
    fn from(api_error: ApiError) -> Self {
        match &api_error.code {
            ApiErrorEntity::Generic(e) => match e {
                GenericError::InternalServerError => OutputError::Generic(GenericOutputError {
                    code: Some("server.internal_error".to_string()),
                    message: format!("Internal server error. Please try again later."),
                    hint: None,
                }),
                GenericError::ServerTemporaryUnavailable => OutputError::Generic(GenericOutputError {
                    code: Some("server.temporary_unavailable".to_string()),
                    message: format!("API service is temporarily unavailable. Please try again later."),
                    hint: None,
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
                GenericError::UnsupportedApiKeyType => {
                    let hint = if let Some(d) = api_error.details {
                        let details = serde_json::from_value::<UnsupportedApiKeyErrorDetails>(d);
                        match details {
                            Ok(details) => Some(format!(
                                "Supported API Key types for this action: {}.",
                                details.supported_api_key_types.join(", ")
                            )),
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    OutputError::Generic(GenericOutputError {
                        message: format!("Current API Key type is not supported for this action."),
                        code: Some("access.unsupported_api_key_type".to_string()),
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
                WebhookError::WebhookLimitReached => OutputError::Generic(GenericOutputError {
                    message: format!("Webhook limit reached."),
                    code: Some(format!("quota.webhook_limit_reached")),
                    hint: Some(format!("Environment reached the maximum number of webhooks allowed.")),
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
            ApiErrorEntity::Scan(e) => match e {
                ScanError::ScanRpmLimitReached => OutputError::Generic(GenericOutputError {
                    code: Some("rate_limit.scan_rpm_limit_reached".to_string()),
                    message: "You have reached the maximum allowed scan requests per minute. Please wait and try again in about a minute.".to_string(),
                    hint: None,
                }),
                ScanError::ScanRequestTooLarge => OutputError::Generic(GenericOutputError {
                    code: Some("rate_limit.scan_request_too_large".to_string()),
                    message: "Scan request exceeds direct scan limits (max 200 files, 10,000 diff lines, 1 MB diff content). Please split the scan into smaller requests.".to_string(),
                    hint: None,
                }),
                ScanError::InvalidIgnoredSecretRegex => {
                    let message = api_error.message.unwrap_or(String::from("Invalid ignored secret regex."));

                    OutputError::Generic(GenericOutputError {
                        code: Some(format!("validation.invalid_ignored_secret_regex")),
                        message,
                        hint: None,
                    })
                }
                ScanError::InvalidIgnoredSecretHash => {
                    let message = api_error.message.unwrap_or(String::from("Invalid ignored secret hash."));

                    OutputError::Generic(GenericOutputError {
                        code: Some(format!("validation.invalid_ignored_secret_hash")),
                        message,
                        hint: None,
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

        if is_color_enabled(false) {
            writeln!(f, "{}", "API Error".red().bold())?;
        } else {
            writeln!(f, "{}", "API Error")?;
        }

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
