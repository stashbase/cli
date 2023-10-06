use core::fmt;

use owo_colors::OwoColorize;
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug)]
pub struct GetRequestArgs {
    pub token: String,
    pub path: ApiPath,
}

#[derive(Debug)]
pub enum ApiPath {
    Projects(Option<String>),
}

impl fmt::Display for ApiPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiPath::Projects(p) => match p {
                Some(value) => write!(f, "projects/{}", value),
                None => write!(f, "projects"),
            },
        }
    }
}

#[derive(Debug)]
pub struct GetRequestApiResponseOk {
    pub status: StatusCode,
    pub text: String,
}

#[derive(Debug)]
pub enum GetRequestApiResponse {
    Ok(GetRequestApiResponseOk),
    Err(CustomError),
}

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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectError {
    InvalidName,
    ProjectAlreadyExists,
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
                ProjectError::ProjectAlreadyExists => CustomError {
                    message: format!("Project already exists"),
                    hint: Some(format!("Try a different name")),
                },
            },
        }
    }
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", "Error".red().bold())?;

        writeln!(f, "- message: {}", self.message).and_then(|_| {
            if let Some(hint) = &self.hint {
                writeln!(f, "- hint: {}", hint)
            } else {
                Ok(())
            }
        })
    }
}
