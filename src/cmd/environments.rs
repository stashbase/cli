use clap::{Args, Subcommand};

#[derive(Debug, Args)]

pub struct EnvironmentCommands {
    /// Project name
    #[arg(value_enum, short = 'p', required = true)]
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

    /// Open environment in browser
    #[clap(alias = "o")]
    Open(GetEnvironment),
}

#[derive(Debug, Args)]
pub struct ListEnvironments {}

#[derive(Debug, Args)]
pub struct GetEnvironment {
    /// Environment name
    pub name: String,
    // /// Project name
    // #[arg(value_enum, short = 'p', required = true)]
    // pub project: String,
}
