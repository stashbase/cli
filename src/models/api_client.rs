use core::fmt;

use owo_colors::OwoColorize;
use reqwest::StatusCode;
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
                Some(value) => write!(f, "projects/{}", value),
                None => write!(f, "projects"),
            },
            ApiPath::Environments { project, path } => match path {
                Some(value) => write!(f, "projects/{}/environments/{}", project, value),
                None => write!(f, "projects/{}/environments", project),
            },
            ApiPath::Secrets {
                project,
                environment,
                path,
            } => match path {
                Some(p) => write!(
                    f,
                    "projects/{}/environments/{}/secrets/{}",
                    project, environment, p
                ),
                None => write!(
                    f,
                    "projects/{}/environments/{}/secrets",
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
                        "projects/{}/environments/{}/changelog/{}",
                        project, environment, p
                    )
                }
                None => {
                    write!(
                        f,
                        "projects/{}/environments/{}/changelog",
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
                        "projects/{}/environments/{}/webhooks/{}",
                        project, environment, p
                    )
                }
                None => {
                    write!(
                        f,
                        "projects/{}/environments/{}/webhooks",
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

// NOTE: POST
#[derive(Debug)]
pub enum PostPatchRequestApiResponse {
    Ok(PostPatchApiResponseOk),
    Err(CustomError),
}

#[derive(Debug)]
pub struct PostPatchApiResponseOk {
    pub status: StatusCode,
    pub text: Option<String>,
}

// NOTE: DELETE
#[derive(Debug)]
pub enum DeleteRequestApiResponse {
    Ok(DeleteApiResponseOk),
    Err(CustomError),
}

pub type DeleteApiResponseOk = PostPatchApiResponseOk;

// error

#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub code: ApiErrorEntity,
    // now onl for env chagnelog - max page
    pub details: Option<String>,
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
            },
            ApiErrorEntity::EnvChangelog(e) => match e {
                EnvChangelogError::PageNotFound => CustomError {
                    message: format!("page not found"),
                    hint: api_error.details,
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
