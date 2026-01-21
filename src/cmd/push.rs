use clap::Args;

use super::pull::PullFormat;
use crate::models::{
    scope::Scope,
    validation::{CmdArgInputValidationError, InputValidationError},
};

pub type PushFormat = PullFormat;

#[derive(Debug, Args)]
#[command(override_usage = "push [OPTIONS]")]
pub struct PushCommand {
    /// Relative path to a config file (default: stashbase.yaml)
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Scope
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,

    /// Target file path if not specified in the config
    #[arg(long = "file")]
    pub file: Option<String>,

    /// Target file format (autodetected by default)
    #[arg(value_enum, long = "format")]
    pub format: Option<PushFormat>,

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

    /// Ignore secret comments
    #[arg(long = "ignore-comments")]
    pub ignore_comments: Option<bool>,
}

impl PushCommand {
    pub fn validate_scope_conflicts(&self) -> Result<(), InputValidationError> {
        if let Some(scope) = &self.scope {
            // Only restrict config file when using environment scope
            // Workspace scope behaves like no scope (allows config file)
            if *scope == Scope::Environment {
                // If environment scope is provided, don't allow config file flag
                if self.config_file.is_some() {
                    return Err(InputValidationError::CmdArgs(
                        CmdArgInputValidationError::ConflictingScopeAndConfigFile,
                    ));
                }

                // Environment scope requires --file flag since no config file
                if self.file.is_none() {
                    return Err(InputValidationError::CmdArgs(
                        CmdArgInputValidationError::FileRequiredForEnvironmentScope,
                    ));
                }
            } else {
                // Workspace scope - require config file
                if self.config_file.is_none() {
                    return Err(InputValidationError::CmdArgs(
                        CmdArgInputValidationError::ConfigFileRequired,
                    ));
                }
            }
        } else {
            // No scope specified - require config file
            if self.config_file.is_none() {
                return Err(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::ConfigFileRequired,
                ));
            }
        }

        Ok(())
    }
}
