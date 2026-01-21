use clap::Args;

use crate::models::{
    scope::Scope,
    validation::{CmdArgInputValidationError, InputValidationError},
};

#[derive(Debug, Args)]
pub struct SharedProjectEnvArgs {
    /// Project name
    #[arg(
        short = 'p',
        long = "project",
        required = false,
        hide = false,
        hide_long_help = false
    )]
    pub project: Option<String>,

    /// Environment name
    #[arg(
        short = 'e',
        long = "environment",
        required = false,
        hide = false,
        hide_long_help = false
    )]
    pub environment: Option<String>,
}

#[derive(Debug, Args)]
pub struct SharedScopeArgs {
    /// Scope
    #[arg(long = "scope", value_enum, hide = false, hide_long_help = false)]
    pub scope: Option<Scope>,
}

#[derive(Debug, Args)]
pub struct SharedProjectEnvScopeArgs {
    #[clap(flatten)]
    pub project_env: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,
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

pub fn try_get_scope(
    root_scope: Option<&Scope>,
    subcommand_scope: Option<&Scope>,
) -> Result<Option<Scope>, InputValidationError> {
    if root_scope.is_some() && subcommand_scope.is_some() {
        let error = InputValidationError::CmdArgs(CmdArgInputValidationError::DuplicateScope);
        return Err(error);
    }

    Ok(root_scope.cloned().or_else(|| subcommand_scope.cloned()))
}
