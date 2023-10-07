use clap::{Args, Subcommand};

#[derive(Debug, Args)]

pub struct EnvironmentCommands {
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

    /// Open environment in browser
    #[clap(alias = "o")]
    Open(GetEnvironment),
}

#[derive(Debug, Args)]
pub struct ListEnvironments {
    /// Project name
    #[arg(value_enum, short = 'p', required = true)]
    pub project: String,
}

#[derive(Debug, Args)]
pub struct GetEnvironment {
    /// Environment name
    pub name: String,

    /// Project name
    #[arg(value_enum, short = 'p', required = true)]
    pub project: String,
}
