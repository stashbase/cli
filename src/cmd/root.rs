use clap::{Parser, Subcommand};

use super::{
    configs::ConfigCommands, environments::EnvironmentCommands, load::LoadCommand,
    projects::ProjectCommands, secrets::SecretArgs,
};

#[derive(Debug, Parser)]
#[command(author, version)]
#[command(about = "Env ease CLI")]

pub struct Cli {
    /// Output data as raw json
    #[arg(long = "raw", global = true)]
    pub raw: bool,

    #[clap(subcommand)]
    pub entity_type: EntityType,
}

#[derive(Debug, Subcommand)]
pub enum EntityType {
    /// Load environment
    Load(LoadCommand),

    #[clap(name = "projects", aliases = &["p", "pro", "proj"])]
    /// Manage projects
    Project(ProjectCommands),

    /// Manage environments
    #[clap(name = "environments", aliases = &["e", "env"])]
    Environment(EnvironmentCommands),

    /// Manage secrets
    #[clap(name = "secrets", aliases = &["s", "sec"])]
    Secret(SecretArgs),

    /// Your CLI configuration
    #[clap(aliases = &["c", "conf"])]
    Config(ConfigCommands),
}
