use anyhow::{bail, Result};
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

pub fn try_get_project_environment(
    root_project: Option<&str>,
    root_environment: Option<&str>,
    // from subcommand
    project: Option<&str>,
    environment: Option<&str>,

    state_project: &Option<String>,
    state_environment: &Option<String>,
) -> Result<(String, String)> {
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
        if state_project.is_none() && state_environment.is_none() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingProjectEnvironment
            ))
        }
    }

    if project.is_none() && root_project.is_none() {
        if state_project.is_none() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingProject
            ))
        }
    }

    if environment.is_none() && root_environment.is_none() {
        if state_environment.is_none() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingEnvironment
            ))
        }
    }

    let project = match root_project {
        Some(p) => p.to_string(),
        // None => project.unwrap().to_string(),
        None => match project {
            Some(p) => p.to_string(),
            None => match state_project {
                Some(p) => p.to_string(),
                None => project.unwrap().to_string(),
            },
        },
    };

    let environment = match root_environment {
        Some(e) => e.to_string(),
        // None => environment.unwrap().to_string(),
        None => match environment {
            Some(e) => e.to_string(),
            None => match state_environment {
                Some(e) => e.to_string(),
                None => environment.unwrap().to_string(),
            },
        },
    };

    Ok((project, environment))
}
