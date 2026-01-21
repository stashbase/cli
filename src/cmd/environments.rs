use core::fmt;
use std::default::Default;

use anyhow::{bail, Result};
use clap::{Args, Subcommand, ValueEnum};

use super::{config::OutputFormat, secrets::SecretsFileFormat};
use crate::models::{
    scope::Scope,
    validation::{CmdArgInputValidationError, InputValidationError},
};

#[derive(Debug, Args)]
#[command(override_usage = "environments <COMMAND> -p <PROJECT> [OPTIONS]")]
pub struct EnvironmentCommands {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    #[clap(subcommand)]
    pub subcommand: EnvironmentSubcommand,
}

#[derive(Debug, Args)]
pub struct SharedProjectArgs {
    /// Project name
    #[arg(
        short = 'p',
        long = "project",
        hide = true,
        required = false,
        hide_long_help = true
    )]
    pub project: Option<String>,
}

// impl RequiredProjectArg for EnvironmentCommands {
impl EnvironmentCommands {
    pub fn try_get_project(&self) -> Result<String> {
        let root_project: Option<_> = self.project.as_deref();
        let project = self.subcommand.get_project();

        if root_project.is_some() && project.is_some() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::DuplicateProject
            ))
        }

        if project.is_none() && root_project.is_none() {
            bail!(InputValidationError::CmdArgs(
                CmdArgInputValidationError::MissingProject
            ))
        }

        match project {
            Some(p) => Ok(p.to_string()),
            None => Ok(root_project.unwrap().to_string()),
        }
    }
}

impl EnvironmentSubcommand {
    fn get_project(&self) -> Option<&String> {
        match self {
            EnvironmentSubcommand::List(l) => l.shared_args.project.as_ref(),
            EnvironmentSubcommand::Get(g) => g.shared_args.project.as_ref(),
            EnvironmentSubcommand::Create(c) => c.shared_args.project.as_ref(),
            EnvironmentSubcommand::Update(u) => u.shared_args.project.as_ref(),
            EnvironmentSubcommand::Compare(c) => c.shared_args.project.as_ref(),
            EnvironmentSubcommand::Delete(d) => d.shared_args.project.as_ref(),
            EnvironmentSubcommand::Open(o) => o.shared_args.project.as_ref(),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum EnvironmentSubcommand {
    /// List environments
    List(ListEnvironments),

    /// Get environment
    Get(GetEnvironment),

    /// Create new environment
    #[clap(aliases = &["new"])]
    Create(CreateEnvironment),

    /// Update environment
    Update(UpdateEnvironment),

    /// Compare secrets of two environments
    Compare(CompareEnvironment),

    /// Delete a project
    #[clap(aliases = &["del"])]
    Delete(DeleteEnvironment),

    /// Open environment in browser
    Open(OpenEnvironment),
}

#[derive(Debug, Args)]
#[command(override_usage = "environments list -p <PROJECT> [OPTIONS]")]
pub struct ListEnvironments {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Search environments by name
    #[arg(long = "search")]
    pub search: Option<String>,

    /// Filter by production status
    #[arg(
        long = "production",
        alias = "prod",
        help = "Filter environments by production status (true/false)"
    )]
    pub is_production: Option<bool>,

    /// Sort environments by
    #[arg(long = "sort-by")]
    pub sort_by: Option<EnvSortBy>,

    /// Descending order
    #[arg(long = "desc")]
    pub descending: bool,

    /// Format output
    #[arg(short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

impl Default for EnvSortBy {
    fn default() -> Self {
        EnvSortBy::Name
    }
}

#[derive(Debug, ValueEnum, Clone)]
pub enum EnvSortBy {
    Name,
    #[value(name = "created_at")]
    CreatedAt,
    #[value(name = "secret_count")]
    SecretCount,
}

impl fmt::Display for EnvSortBy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EnvSortBy::Name => write!(f, "name"),
            EnvSortBy::CreatedAt => write!(f, "created_at"),
            EnvSortBy::SecretCount => write!(f, "secret_count"),
        }?;

        Ok(())
    }
}

#[derive(Debug, Args)]
#[command(
    override_usage = "environments get (<NAME_OR_ID> -p <PROJECT> | --scope=environment) [OPTIONS]"
)]
pub struct GetEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: Option<String>,

    /// API scope [default: workspace]
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,

    /// Format output
    #[arg(short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "environments delete <NAME_OR_ID> -p <PROJECT> [OPTIONS]")]
pub struct DeleteEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// Proceed without confirmation
    #[arg(short = 'f', long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "environments open <NAME_OR_ID> -p <PROJECT> [OPTIONS]")]
pub struct OpenEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: Option<String>,

    /// Scope
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,
}

#[derive(Debug, Args)]
#[command(override_usage = "environments update <NAME_OR_ID> -p <PROJECT> [OPTIONS]")]
pub struct UpdateEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// New environment name
    #[arg(short = 'n', long = "name")]
    pub new_name: Option<String>,

    /// Environment description
    #[arg(short = 'd', long = "description")]
    pub description: Option<String>,

    /// Whether the environment is production or not, defaults to false
    #[arg(name = "production", alias = "prod", long = "production")]
    pub is_production: Option<bool>,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "environments duplicate <NAME_OR_ID> <NEW_NAME> -p <PROJECT> [OPTIONS]")]
pub struct DuplicateEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// New name
    pub new_name: String,
}

#[derive(Debug, Args)]
#[command(
    override_usage = "environments compare <NAME_OR_ID_1> <NAME_OR_ID_2> -p <PROJECT> [OPTIONS]"
)]
pub struct CompareEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name or id
    #[arg(value_name = "NAME_OR_ID_1")]
    pub identifier_1: String,

    /// Environment name or id to compare with
    #[arg(value_name = "NAME_OR_ID_2")]
    pub identifier_2: String,

    /// Return secrets with values
    #[arg(long = "with-values")]
    pub with_values: bool,

    /// Expand secrets references to their values
    #[arg(long = "expand-refs")]
    pub expand_refs: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "environments create <NAME> --type <TYPE> -p <PROJECT> [OPTIONS]")]
pub struct CreateEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name
    pub name: String,

    /// Whether the environment is production or not, defaults to false
    #[arg(name = "production", alias = "prod", long = "production")]
    pub is_production: Option<bool>,

    /// Environment description
    #[arg(short = 'd', long = "description")]
    pub description: Option<String>,

    /// Add with secrets - path to file
    #[arg(short = 'f', long = "file")]
    pub file_path: Option<String>,

    /// Secrets file format (if file provided)
    #[arg(long = "format")]
    pub file_format: Option<SecretsFileFormat>,

    /// Open environment in browser
    #[arg(long = "open")]
    pub open: bool,
}
