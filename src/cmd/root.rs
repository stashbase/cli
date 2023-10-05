use clap::{Parser, Subcommand};

use super::projects::ProjectCommands;

#[derive(Debug, Parser)]
#[command(author, version)]
#[command(about = "Env ease CLI")]

pub struct Cli {
    #[clap(subcommand)]
    pub entity_type: Option<EntityType>,
}

#[derive(Debug, Subcommand)]
pub enum EntityType {
    #[clap(name = "projects")]
    /// Manage projects
    Project(ProjectCommands),
}
