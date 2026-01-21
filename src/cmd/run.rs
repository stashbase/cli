use clap::Args;

use crate::models::{
    scope::Scope,
    secrets::PrintSecrets,
    validation::{CmdArgInputValidationError, InputValidationError},
};

#[derive(Debug, Args)]
#[command(override_usage = "run [OPTIONS] [COMMAND]...")]
pub struct RunCommand {
    /// Command to run
    #[clap(num_args = 1..)]
    pub command: Vec<String>,

    /// Relative path to a config file (default: stashbase.yaml)
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Project name or id
    #[arg(short = 'p', long = "project")]
    pub project: Option<String>,

    /// Enviornment name or id
    #[arg(short = 'e', long = "environment")]
    pub environment: Option<String>,

    /// Select secret names
    #[clap(long="only", num_args = 1..)]
    pub only: Vec<String>,

    /// Exclude secret names
    #[clap(long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

    /// Manually set secrets
    #[clap(long="set", num_args = 1..)]
    pub set: Vec<String>,

    /// Expand references to their values
    #[arg(long = "expand-refs")]
    pub expand_refs: Option<bool>,

    /// Print loaded secrets
    #[arg(value_enum, long = "print-secrets")]
    pub print_secrets: Option<PrintSecrets>,

    /// Scope
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,
}

impl RunCommand {
    pub fn validate_scope_conflicts(&self) -> Result<(), InputValidationError> {
        if let Some(scope) = &self.scope {
            // Only restrict flags when using environment scope
            // Workspace scope behaves like no scope (allows project/environment/config)
            if *scope == Scope::Environment {
                // If environment scope is provided, don't allow project and environment flags
                if self.project.is_some() || self.environment.is_some() {
                    return Err(InputValidationError::CmdArgs(
                        CmdArgInputValidationError::ConflictingScopeAndProjectEnvironment,
                    ));
                }

                // If environment scope is provided, don't allow config file flag
                if self.config_file.is_some() {
                    return Err(InputValidationError::CmdArgs(
                        CmdArgInputValidationError::ConflictingScopeAndConfigFile,
                    ));
                }
            }
        }

        Ok(())
    }
}
