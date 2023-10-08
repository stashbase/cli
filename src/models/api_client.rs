use core::fmt;

use owo_colors::OwoColorize;
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug)]
pub struct RequestArgs {
    pub token: String,
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
    Secrets {
        project: String,
        environment: String,
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
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ApiErrorEntity {
    Project(ProjectError),
    Environment(EnvironmentError),
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectError {
    InvalidName,
    ProjectAlreadyExists,
    ProjectNotFound,
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
                ProjectError::ProjectAlreadyExists => CustomError {
                    message: format!("project already exists"),
                    hint: Some(format!("use a different name")),
                },
            },
            ApiErrorEntity::Environment(e) => match e {
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
        }
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
