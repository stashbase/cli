use anyhow::bail;
use clap::Args;

use crate::models::validation::{CmdArgInputValidationError, InputValidationError};

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

pub trait RequiredArgs {
    // fn try_get_project_environment(&self) -> anyhow::Result<(String, String)>;
    fn try_get_project_environment(&self) -> anyhow::Result<(String, String)>;
}

pub trait RequiredProjectArg {
    fn try_get_project(&self) -> anyhow::Result<String>;
}

pub fn try_get_project_environment(
    root_project: Option<&str>,
    root_environment: Option<&str>,
    // from subcommand
    project: Option<String>,
    environment: Option<String>,
) -> anyhow::Result<(String, String)> {
    if root_project.is_some() && project.is_some() {
        bail!(InputValidationError::CmdArgs(
            CmdArgInputValidationError::DuplicateProject
        ))
    }

    if root_environment.is_some() && environment.is_some() {
        bail!(InputValidationError::CmdArgs(
            CmdArgInputValidationError::DuplicateEnvironment
        ))
    }

    if project.is_none()
        && root_project.is_none()
        && environment.is_none()
        && root_environment.is_none()
    {
        bail!(InputValidationError::CmdArgs(
            CmdArgInputValidationError::MissingProjectEnvironment
        ))
    }

    if project.is_none() && root_project.is_none() {
        bail!(InputValidationError::CmdArgs(
            CmdArgInputValidationError::MissingProject
        ))
    }

    if environment.is_none() && root_environment.is_none() {
        bail!(InputValidationError::CmdArgs(
            CmdArgInputValidationError::MissingEnvironment
        ))
    }

    let project = match root_project {
        Some(p) => p.to_string(),
        None => project.unwrap(),
    };

    let environment = match root_environment {
        Some(e) => e.to_string(),
        None => environment.unwrap(),
    };

    Ok((project, environment))
}
