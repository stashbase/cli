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
    List(ListProjects),

    /// Get project
    Get(GetProject),

    /// Create a new project
    #[clap(aliases = &["new"])]
    Create(CreateProject),

    /// Update project
    Update(UpdateProject),

    /// Delete a project
    #[clap(aliases = &["del"])]
    Delete(DeleteProject),

    /// Open project in browser
    Open(OpenProject),
}

#[derive(Debug, Args)]
#[command(override_usage = "projects list [OPTIONS]")]
pub struct ListProjects {
    /// Search projects by name
    #[arg(long = "search")]
    pub search: Option<String>,

    /// Sort projects by
    #[arg(long = "sort-by")]
    pub sort_by: Option<SortBy>,

    /// Descending order
    #[arg(long = "desc")]
    pub descending: bool,

    /// Page (selected page)
    #[arg(long = "page")]
    pub page: Option<usize>,

    /// Number of items per page
    #[arg(long = "page-size")]
    pub page_size: Option<usize>,

    /// Format output
    #[arg(short = 'f', long = "format")]
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
    #[value(name = "created_at")]
    CreatedAt,
    #[value(name = "environment_count")]
    EnvironmentCount,
}

impl fmt::Display for SortBy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SortBy::Name => write!(f, "name"),
            SortBy::CreatedAt => write!(f, "created_at"),
            SortBy::EnvironmentCount => write!(f, "environment_count"),
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
    #[arg(short = 'd', long = "description")]
    pub description: Option<String>,

    /// Open project in browser
    #[arg(long = "open")]
    pub open: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects update <NAME_OR_ID> [OPTIONS]")]
pub struct UpdateProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// New name
    #[arg(short = 'n', long = "name")]
    pub new_name: Option<String>,

    /// Project description
    #[arg(short = 'd', long = "description")]
    pub description: Option<String>,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects get <NAME_OR_ID> [OPTIONS]")]
pub struct GetProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// Format output
    #[arg(short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects delete <NAME_OR_ID> [OPTIONS]")]
pub struct DeleteProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects open <NAME_OR_ID> [OPTIONS]")]
pub struct OpenProject {
    /// Project name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub identifier: String,
}
