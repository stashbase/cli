use std::fmt::Display;

use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::models::validation::{CmdArgInputValidationError, InputValidationError};

#[derive(Debug, ValueEnum, Default, Clone, PartialEq, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    Auto,
    Workspace,
    Environment,
}

impl Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Workspace => write!(f, "workspace"),
            Self::Environment => write!(f, "environment"),
        }
    }
}
#[derive(Debug, Args)]
pub struct SharedProjectEnvArgs {
    /// Project name
    #[arg(
        short = 'p',
        long = "project",
        required = false,
        hide = true,
        hide_long_help = true
    )]
    pub project: Option<String>,

    /// Environment name
    #[arg(
        short = 'e',
        long = "environment",
        required = false,
        hide = true,
        hide_long_help = true
    )]
    pub environment: Option<String>,
}

pub fn try_get_project_environment(
    root_project: Option<&str>,
    root_environment: Option<&str>,
    // from subcommand
    project: Option<&str>,
    environment: Option<&str>,
) -> Result<(String, String), InputValidationError> {
    if root_project.is_some() && project.is_some() {
        let error = InputValidationError::CmdArgs(CmdArgInputValidationError::DuplicateProject);
        return Err(error);
    }

    if root_environment.is_some() && environment.is_some() {
        let error = InputValidationError::CmdArgs(CmdArgInputValidationError::DuplicateEnvironment);
        return Err(error);
    }

    if project.is_none()
        && root_project.is_none()
        && environment.is_none()
        && root_environment.is_none()
    {
        let error =
            InputValidationError::CmdArgs(CmdArgInputValidationError::MissingProjectEnvironment);

        return Err(error);
    }

    if project.is_none() && root_project.is_none() {
        let error = InputValidationError::CmdArgs(CmdArgInputValidationError::MissingProject);
        return Err(error);
    }

    if environment.is_none() && root_environment.is_none() {
        let error = InputValidationError::CmdArgs(CmdArgInputValidationError::MissingEnvironment);
        return Err(error);
    }

    let project = match root_project {
        Some(p) => p.to_string(),
        None => project.unwrap().to_string(),
    };

    let environment = match root_environment {
        Some(e) => e.to_string(),
        None => environment.unwrap().to_string(),
    };

    Ok((project, environment))
}
