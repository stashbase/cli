use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};

use super::config::SecretsOutputFormat;
use crate::models::{
    scope::Scope,
    secrets::PrintSecrets,
    validation::{CmdArgInputValidationError, InputValidationError},
};

#[derive(Serialize, Deserialize, Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum PullFormat {
    #[serde(rename = "dotenv")]
    Dotenv,
    #[serde(rename = "yaml")]
    Yaml,
    #[serde(rename = "json")]
    Json,
}

impl TryFrom<PullFormat> for SecretsOutputFormat {
    type Error = ();

    fn try_from(pf: PullFormat) -> Result<SecretsOutputFormat, Self::Error> {
        match pf {
            PullFormat::Dotenv => Ok(SecretsOutputFormat::Dotenv),
            PullFormat::Yaml => Ok(SecretsOutputFormat::Yaml),
            PullFormat::Json => Ok(SecretsOutputFormat::Json),
        }
    }
}

#[derive(Debug, Args)]
#[command(override_usage = "pull [OPTIONS]")]
pub struct PullCommand {
    /// Relative path to a config file (default: stashbase.yaml)
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// API scope [default: workspace]
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,

    /// Target file path if not specified in the config
    #[arg(long = "file")]
    pub file: Option<String>,

    /// Target file format (autodetected by default)
    #[arg(value_enum, long = "format")]
    pub format: Option<PullFormat>,

    /// Overwrite existing file without prompt
    #[arg(long = "overwrite")]
    pub overwrite: bool,

    // /// Project name
    // #[arg(value_enum, short = 'p', long = "project")]
    // pub project: Option<String>,
    //
    // /// Enviornment name
    // #[arg(value_enum, short = 'e', long = "environment")]
    // pub environment: Option<String>,
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

    /// Print pulled secrets
    #[arg(value_enum, long = "print-secrets")]
    pub print_secrets: Option<PrintSecrets>,
}

impl PullCommand {
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
