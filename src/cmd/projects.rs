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

    /// Get project
    #[clap(alias = "g")]
    Get(GetProject),

    /// Create a new project
    #[clap(aliases = &["c", "new"])]
    Create(CreateProject),

    /// Update project
    #[clap(aliases = &["u", "upd"])]
    Update(UpdateProject),

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
pub struct UpdateProject {
    /// Project name
    pub name: String,

    /// New name
    #[arg(value_enum, short = 'n', long = "name")]
    pub new_name: Option<String>,

    /// Project description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
pub struct GetProject {
    /// Project name
    pub name: String,
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
