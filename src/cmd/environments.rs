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
}

#[derive(Debug, Args)]
pub struct ListEnvironments {
    /// Project name
    #[arg(value_enum, short = 'p', required = true)]
    pub project: String,
}
