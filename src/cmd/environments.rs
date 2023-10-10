use core::fmt;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, ValueEnum, Clone)]
pub enum EnvironmentType {
    Development,
    Testing,
    Staging,
    Production,
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

    /// Open environment in browser
    #[clap(alias = "o")]
    Open(GetEnvironment),
}

#[derive(Debug, Args)]
// TODO: order/group by type + locked ???
pub struct ListEnvironments {
    /// Sort projects by
    #[arg(value_enum, short = 's', long = "sort")]
    pub sort: Option<EnvSort>,

    /// Descending order
    #[arg(value_enum, long = "desc")]
    pub descending: bool,
}

#[derive(Debug, ValueEnum, Clone)]
pub enum EnvSort {
    #[clap(alias = "cre")]
    Created,

    #[clap(alias = "alp")]
    Alphabet,

    #[clap(alias = "sec")]
    Secrets,

    Lock,
}

impl fmt::Display for EnvSort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EnvSort::Created => write!(f, "created"),
            EnvSort::Alphabet => write!(f, "alphabet"),
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
