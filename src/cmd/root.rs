use clap::{Parser, Subcommand};

use super::{configs::ConfigCommands, projects::ProjectCommands};

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
    #[clap(name = "projects", aliases = &["p", "pro", "proj"])]
    /// Manage projects
    Project(ProjectCommands),
    /// Your CLI configuration
    #[clap(aliases = &["c", "conf"])]
    Config(ConfigCommands),
}
