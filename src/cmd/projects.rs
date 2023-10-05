use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ProjectCommands {
    #[clap(subcommand)]
    pub subcommand: ProjectSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectSubcommand {
    /// Manage projects
    List(ListProjects),
}

#[derive(Debug, Args)]
// TODO: perPage, pages
pub struct ListProjects {}
