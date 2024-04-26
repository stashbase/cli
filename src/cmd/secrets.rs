use anyhow::{bail, Result};
use clap::{Args, Subcommand, ValueEnum};

use crate::models::validation::{CmdArgInputValidationError, InputValidationError};

#[derive(Debug, Args)]
pub struct SecretArgs {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,

    #[clap(subcommand)]
    pub subcommand: SecretSubcommand,
}

// TODO: error
impl SecretArgs {
    pub fn try_get_project_environment(&self) -> Result<(String, String)> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

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

        return Ok((project, environment));
    }
}

impl SecretSubcommand {
    pub fn get_project_environment(&self) -> (Option<String>, Option<String>) {
        match &self {
            SecretSubcommand::List(l) => (
                l.shared_args.project.to_owned(),
                l.shared_args.environment.to_owned(),
            ),
            SecretSubcommand::Get(g) => (
                g.shared_args.project.to_owned(),
                g.shared_args.environment.to_owned(),
            ),
            SecretSubcommand::Set(s) => (
                s.shared_args.project.to_owned(),
                s.shared_args.environment.to_owned(),
            ),
            SecretSubcommand::Upload(u) => (
                u.shared_args.project.to_owned(),
                u.shared_args.environment.to_owned(),
            ),
            SecretSubcommand::Rename(r) => (
                r.shared_args.project.to_owned(),
                r.shared_args.environment.to_owned(),
            ),
            SecretSubcommand::Description(d) => (
                d.shared_args.project.to_owned(),
                d.shared_args.environment.to_owned(),
            ),
            SecretSubcommand::Delete(d) => (
                d.shared_args.project.to_owned(),
                d.shared_args.environment.to_owned(),
            ),
        }
    }
}

/// Common options for listing secrets
#[derive(Debug, Args)]
pub struct SharedArgs {
    /// Project name
    #[arg(short = 'p', long = "project", required = false, hide = true)]
    pub project: Option<String>,

    /// Environment name
    #[arg(short = 'e', long = "environment", required = false, hide = true)]
    pub environment: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum SecretSubcommand {
    /// List secrets
    #[clap(alias = "l")]
    List(ListSecrets),

    /// Get secrets
    #[clap(alias = "g")]
    Get(GetSecrets),

    /// Set secrets
    #[clap(alias = "s")]
    Set(SetSecrets),

    /// Upload secrets
    #[clap(alias = "upl")]
    Upload(UploadSecrets),

    /// Rename secrets
    #[clap(alias = "r")]
    Rename(RenameSecrets),

    /// Set description of existing secret
    #[clap(alias = "des")]
    Description(SetDescription),

    /// Delete one or multiple secrets
    #[clap(aliases = &[ "del"])]
    Delete(DeleteSecrets),
}

#[derive(Debug, Args)]
pub struct ListSecrets {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    /// Search key
    #[arg(value_enum, long = "search")]
    pub search: Option<String>,

    /// Project description
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,

    /// Print only keys
    #[arg(value_enum, long = "only-keys")]
    pub only_keys: bool,
}

#[derive(Debug, Args)]
pub struct GetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    // #[clap(short='v', long="k", value_parser, num_args = 1.., value_delimiter = ' ')]
    pub keys: Vec<String>,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,
}

#[derive(Debug, Args)]
pub struct DeleteSecrets {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    /// Secrets (keys) to delete
    #[clap(value_parser, num_args = 1.., value_delimiter = ' ')]
    pub keys: Vec<String>,

    /// Delete all secrets
    #[arg(name = "all", value_enum, long = "all")]
    pub delete_all: bool,
}

#[derive(Debug, Args)]
pub struct SetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    /// Secrets to set: KEY_1=VAL_1 KEY_2=VAL_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,

    /// Descriptions to set: KEY_1=NOTE_1 KEY_2=NOTE_2
    // #[clap(value_parser, long="description", short='d', num_args = 1.., value_delimiter = ' ')]
    #[clap(value_parser, long="description", short='d', num_args = 1..)]
    pub descriptions: Vec<String>,
}

#[derive(Debug, Args)]
pub struct UploadSecrets {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    // NOTE: for now only accepts .env
    /// Path to file (dotenv format)
    pub file_path: String,
}

#[derive(Debug, Args)]
pub struct SetDescription {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    /// Secret key
    pub key: String,

    /// Description
    pub description: String,
}

#[derive(Debug, Args)]
pub struct RenameSecrets {
    #[clap(flatten)]
    pub shared_args: SharedArgs,

    /// Secrets to rename: KEY_1=NEW_KEY_1 KEY_2=NEW_KEY_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum SecretsFromat {
    List,
    Dotenv,
    Table,
    Json,
}
