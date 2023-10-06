use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ProjectCommands {
    #[clap(subcommand)]
    pub subcommand: ProjectSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectSubcommand {
    /// List projects
    List(ListProjects),
    /// Create a new project
    Create(CreateProject),
    /// Delete a project
    Delete(DeleteProject),
}
#[derive(Debug, Args)]
// TODO: perPage, pages + other args
pub struct ListProjects {}

#[derive(Debug, Args)]
pub struct CreateProject {
    /// Project name
    pub name: String,

    /// Project description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
pub struct DeleteProject {
    /// Project name
    pub name: String,
}
