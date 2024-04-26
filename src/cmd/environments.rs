use core::fmt;

use anyhow::{bail, Result};
use clap::{Args, Subcommand, ValueEnum};

use super::shared::SharedProjectEnvArgs;
use crate::models::validation::{CmdArgInputValidationError, InputValidationError};

#[derive(Debug, ValueEnum, Clone)]
pub enum EnvironmentType {
    #[clap(alias = "dev")]
    Development,

    #[clap(alias = "test")]
    Testing,

    #[clap(alias = "stg")]
    Staging,

    #[clap(alias = "prod")]
    Production,
}

impl fmt::Display for EnvironmentType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EnvironmentType::Development => write!(f, "development"),
            EnvironmentType::Testing => write!(f, "testing"),
            EnvironmentType::Staging => write!(f, "staging"),
            EnvironmentType::Production => write!(f, "production"),
        }?;

        Ok(())
    }
}

#[derive(Debug, Args)]
pub struct EnvironmentCommands {
    /// Project name
    #[arg(
        value_enum,
        short = 'p',
        long = "project",
        hide = false,
        required = false,
        hide_long_help = true
    )]
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

    // for changelog
    pub fn try_get_project_environment(&self) -> Result<(String, String)> {
        if let EnvironmentSubcommand::Changelog(c) = &self.subcommand {
            let root_project = self.project.as_deref();
            let subcommand_project = self.subcommand.get_project();

            let nested_subcommand_project = match &c.subcommand {
                EnvChangelogSubcommand::List(l) => l.shared_args.project.as_deref(),
                EnvChangelogSubcommand::Get(g) => g.shared_args.project.as_deref(),
                EnvChangelogSubcommand::Revert(r) => r.shared_args.project.as_deref(),
            };

            let mut project_arg_count = 0;

            if root_project.is_some() {
                project_arg_count += 1
            }

            if subcommand_project.is_some() {
                project_arg_count += 1
            }
            if nested_subcommand_project.is_some() {
                project_arg_count += 1
            }

            // environment
            let subcommand_environment = c.shared_args.environment.as_deref();
            let nested_subcommand_environment = match &c.subcommand {
                EnvChangelogSubcommand::List(l) => l.shared_args.environment.as_deref(),
                EnvChangelogSubcommand::Get(g) => g.shared_args.environment.as_deref(),
                EnvChangelogSubcommand::Revert(r) => r.shared_args.environment.as_deref(),
            };

            // checks
            if project_arg_count > 1 {
                bail!(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::DuplicateProject
                ))
            }

            if subcommand_environment.is_some() && nested_subcommand_environment.is_some() {
                bail!(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::DuplicateEnvironment
                ))
            }

            if project_arg_count == 0 {
                if subcommand_environment.is_none() && nested_subcommand_environment.is_none() {
                    bail!(InputValidationError::CmdArgs(
                        CmdArgInputValidationError::MissingProjectEnvironment
                    ))
                }

                bail!(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::MissingProject
                ))
            }

            if subcommand_environment.is_none() && nested_subcommand_environment.is_none() {
                bail!(InputValidationError::CmdArgs(
                    CmdArgInputValidationError::MissingEnvironment
                ))
            }

            let project = match nested_subcommand_project {
                Some(p) => p.to_string(),
                None => match subcommand_project {
                    Some(p) => p.to_string(),
                    None => root_project.unwrap().to_string(),
                },
            };

            let environment = match nested_subcommand_environment {
                Some(e) => e.to_string(),
                None => subcommand_environment.unwrap().to_string(),
            };

            return Ok((project, environment));
        } else {
            bail!("Changelog subcommand is only supported for changelog command")
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
            EnvironmentSubcommand::Duplicate(d) => d.shared_args.project.as_ref(),
            EnvironmentSubcommand::Compare(c) => c.shared_args.project.as_ref(),
            EnvironmentSubcommand::Lock(l) => l.shared_args.project.as_ref(),
            EnvironmentSubcommand::Unlock(u) => u.shared_args.project.as_ref(),
            EnvironmentSubcommand::SetType(s) => s.shared_args.project.as_ref(),
            EnvironmentSubcommand::Delete(d) => d.shared_args.project.as_ref(),
            EnvironmentSubcommand::Changelog(c) => c.shared_args.project.as_ref(),
            EnvironmentSubcommand::Open(o) => o.shared_args.project.as_ref(),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum EnvironmentSubcommand {
    /// List environments
    #[clap(alias = "l")]
    List(ListEnvironments),

    /// Get environment
    #[clap(alias = "g")]
    Get(GetEnvironment),

    /// Create new environment
    #[clap(aliases = &["c", "new"])]
    Create(CreateEnvironment),

    /// Update environment
    #[clap(alias = "u")]
    Update(UpdateEnvironment),

    /// Duplicate environment
    // #[clap(alias = "d")]
    Duplicate(DuplicateEnvironment),

    Compare(CompareEnvironment),

    /// Lock project
    Lock(GetEnvironment),

    /// Unlock project
    Unlock(GetEnvironment),

    /// Update environment type
    #[clap(aliases = &["s"])]
    SetType(SetType),

    /// Delete a project
    #[clap(aliases = &["d", "del"])]
    Delete(GetEnvironment),

    /// Environment changelog
    Changelog(EnvChangelog),

    /// Open environment in browser
    #[clap(alias = "o")]
    Open(GetEnvironment),
}

#[derive(Debug, Args)]
// TODO: order/group by type + locked ???
pub struct ListEnvironments {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Search name
    #[arg(value_enum, long = "search")]
    pub search: Option<String>,

    /// Filter environment types
    #[arg(value_enum, name = "types", short = 't', long = "types", num_args = 1..)]
    pub types: Vec<EnvironmentType>,

    /// Filter locked
    #[arg(value_enum, long = "locked")]
    pub locked: bool,

    /// Filter unlocked
    #[arg(value_enum, long = "unlocked")]
    pub unlocked: bool,

    /// Sort projects by
    #[arg(value_enum, short = 's', long = "sort")]
    pub sort: Option<EnvSort>,

    /// Descending order
    #[arg(value_enum, long = "desc")]
    pub descending: bool,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<EnvironmentFormat>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default)]
pub enum EnvironmentFormat {
    #[default]
    List,
    Json,
    Table,
}

#[derive(Debug, ValueEnum, Clone)]
pub enum EnvSort {
    #[clap(alias = "cre")]
    Created,
    Name,

    // #[clap(alias = "alp")]
    // Alphabet,
    //
    #[clap(alias = "sec")]
    Secrets,

    Lock,
}

impl fmt::Display for EnvSort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EnvSort::Created => write!(f, "created"),
            EnvSort::Name => write!(f, "name"),
            EnvSort::Secrets => write!(f, "secrets"),
            EnvSort::Lock => write!(f, "lock"),
        }?;

        Ok(())
    }
}

#[derive(Debug, Args)]
pub struct GetEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name
    pub name: String,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<EnvironmentFormat>,
}

#[derive(Debug, Args)]
pub struct UpdateEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name
    pub name: String,

    /// New environment name
    #[arg(value_enum, short = 'n', long = "name")]
    pub new_name: Option<String>,

    /// Environment description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
pub struct DuplicateEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name
    pub name: String,
    /// New name
    pub new_name: String,
}

#[derive(Debug, Args)]
pub struct CompareEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name
    pub name_1: String,

    /// Environment name to compare with
    pub name_2: String,

    /// Return only keys without values
    #[arg(value_enum, long = "only-keys")]
    pub only_keys: bool,
}

#[derive(Debug, Args)]
pub struct CreateEnvironment {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    /// Environment name
    pub name: String,

    /// Environment type
    #[arg(value_enum, name = "type", short = 't', long = "type")]
    pub env_type: EnvironmentType,

    /// Environment description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,

    // NOTE: for now only accepts .env
    /// Add with secrets - path to file (dotenv format)
    #[arg(value_enum, short = 'f', long = "file")]
    pub file_path: Option<String>,

    /// Open environment in browser
    #[arg(value_enum, long = "open")]
    pub open: bool,
}

#[derive(Debug, Args)]
pub struct SetType {
    #[clap(flatten)]
    pub shared_args: SharedProjectArgs,

    pub name: String,

    // #[arg(name = "type")]
    #[arg(value_enum, name = "type", short = 't', long = "type")]
    pub env_type: EnvironmentType,
}

#[derive(Debug, Args)]
pub struct EnvChangelog {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(subcommand)]
    pub subcommand: EnvChangelogSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum EnvChangelogSubcommand {
    /// List changelog records
    #[clap(alias = "l")]
    List(ListChangelog),

    /// List changelog record
    #[clap(alias = "g")]
    Get(GetChangelogItem),

    /// List changelog records
    #[clap(alias = "r")]
    Revert(RevertChangelog),
}

#[derive(Debug, Args)]
pub struct ListChangelog {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // /// Environment name
    // pub name: String,
    /// Show secret values
    #[arg(value_enum, long = "page")]
    pub page: Option<usize>,

    /// Show secret values
    // #[arg(value_enum, long = "only-secrets")]
    // pub only_secrets: bool,

    /// Show secret values
    #[arg(value_enum, long = "show-values")]
    pub show_values: bool,
}

#[derive(Debug, Args)]
pub struct GetChangelogItem {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // /// Environment name
    // pub name: String,

    // #[arg(value_enum, short = 'i', long = "id")]
    /// Item id
    pub id: String,
}

#[derive(Debug, Args)]
pub struct RevertChangelog {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // /// Environment name
    // pub name: String,

    // #[arg(value_enum, short = 'i', long = "id")]
    /// Item id
    pub id: String,
}
