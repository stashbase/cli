use core::fmt;

use clap::{Args, Subcommand, ValueEnum};

use super::config::OutputFormat;

#[derive(Debug, Args)]
#[command(override_usage = "projects <COMMAND> [OPTIONS]")]
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
#[command(override_usage = "projects list [OPTIONS]")]
// TODO: perPage, pages + other args
pub struct ListProjects {
    /// Search name
    #[arg(value_enum, long = "search")]
    pub search: Option<String>,

    /// Sort projects by
    #[arg(value_enum, short = 's', long = "sort-by")]
    pub sort_by: Option<SortBy>,

    /// Descending order
    #[arg(value_enum, long = "desc")]
    pub descending: bool,

    /// Page (selected page)
    #[arg(value_enum, long = "page")]
    pub page: Option<usize>,

    /// Take (number of) items per page
    #[arg(value_enum, long = "limit")]
    pub limit: Option<usize>,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

impl Default for SortBy {
    fn default() -> Self {
        SortBy::Name
    }
}

#[derive(Debug, ValueEnum, Clone)]
pub enum SortBy {
    Name,
    #[value(name = "createdAt")]
    CreatedAt,
    #[value(name = "environmentCount")]
    EnvironmentCount,
}

impl fmt::Display for SortBy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SortBy::Name => write!(f, "name"),
            SortBy::CreatedAt => write!(f, "createdAt"),
            SortBy::EnvironmentCount => write!(f, "environmentCount"),
        }?;

        Ok(())
    }
}

#[derive(Debug, Args)]
#[command(override_usage = "projects create <NAME> [OPTIONS]")]
pub struct CreateProject {
    /// Project name
    pub name: String,

    /// Project description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects update <NAME_OR_ID> [OPTIONS]")]
pub struct UpdateProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// New name
    #[arg(value_enum, short = 'n', long = "name")]
    pub new_name: Option<String>,

    /// Project description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects get <NAME_OR_ID> [OPTIONS]")]
pub struct GetProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects delete <NAME_OR_ID> [OPTIONS]")]
pub struct DeleteProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects open <NAME_OR_ID> [OPTIONS]")]
pub struct OpenProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,
}
