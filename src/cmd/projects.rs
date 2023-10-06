use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ProjectCommands {
    #[clap(subcommand)]
    pub subcommand: ProjectSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectSubcommand {
    /// List projects
    #[clap(alias = "l")]
    List(ListProjects),
    /// Create a new project
    #[clap(aliases = &["c", "new"])]
    Create(CreateProject),
    /// Delete a project
    #[clap(aliases = &["d", "del"])]
    Delete(DeleteProject),

    /// Open project in browser
    #[clap(alias = "o")]
    Open(OpenProject),
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

#[derive(Debug, Args)]
pub struct OpenProject {
    /// Project name
    pub name: String,
}
