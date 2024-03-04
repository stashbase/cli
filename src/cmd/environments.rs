use core::fmt;

use clap::{Args, Subcommand, ValueEnum};

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
    #[arg(value_enum, short = 'p', long = "project", required = true)]
    pub project: String,

    #[clap(subcommand)]
    pub subcommand: EnvironmentSubcommand,
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
    /// Environment name
    pub name: String,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<EnvironmentFormat>,
}

#[derive(Debug, Args)]
pub struct UpdateEnvironment {
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
    /// Environment name
    pub name: String,
    /// New name
    pub new_name: String,
}

#[derive(Debug, Args)]
pub struct CompareEnvironment {
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
    pub name: String,

    // #[arg(name = "type")]
    #[arg(value_enum, name = "type", short = 't', long = "type")]
    pub env_type: EnvironmentType,
}

#[derive(Debug, Args)]
pub struct EnvChangelog {
    /// Environmentname
    #[arg(value_enum, short = 'e', long = "environment", required = true)]
    pub environment: String,

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
    // /// Environment name
    // pub name: String,
    /// Show secret values
    #[arg(value_enum, short = 'p', long = "page")]
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
    // /// Environment name
    // pub name: String,

    // #[arg(value_enum, short = 'i', long = "id")]
    /// Item id
    pub id: String,
}

#[derive(Debug, Args)]
pub struct RevertChangelog {
    // /// Environment name
    // pub name: String,

    // #[arg(value_enum, short = 'i', long = "id")]
    /// Item id
    pub id: String,
}
