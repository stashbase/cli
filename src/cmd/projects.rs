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

impl ProjectCommands {
    pub fn get_name_argument(&self) -> Option<String> {
        match &self.subcommand {
            ProjectSubcommand::Get(g) => g.name.to_owned(),
            ProjectSubcommand::Open(o) => o.name.to_owned(),
            ProjectSubcommand::Delete(d) => d.name.to_owned(),
            ProjectSubcommand::Update(u) => u.name.to_owned(),
            _ => None,
        }
    }
}

#[derive(Debug, Args)]
#[command(override_usage = "projects list [OPTIONS]")]
// TODO: perPage, pages + other args
pub struct ListProjects {
    /// Search name
    #[arg(value_enum, long = "search")]
    pub search: Option<String>,

    /// Sort projects by
    #[arg(value_enum, short = 's', long = "sort")]
    pub sort: Option<Sort>,

    /// Descending order
    #[arg(value_enum, long = "desc")]
    pub descending: bool,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, ValueEnum, Clone)]
pub enum Sort {
    #[clap(alias = "cre")]
    Created,
    Name,
    #[clap(alias = "env")]
    Environments,
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Sort::Created => write!(f, "created"),
            Sort::Name => write!(f, "name"),
            Sort::Environments => write!(f, "environments"),
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
#[command(override_usage = "projects update <NAME> [OPTIONS]")]
pub struct UpdateProject {
    /// Project name
    pub name: Option<String>,

    /// New name
    #[arg(value_enum, short = 'n', long = "name")]
    pub new_name: Option<String>,

    /// Project description
    #[arg(value_enum, short = 'd', long = "description")]
    pub description: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects get <NAME> [OPTIONS]")]
pub struct GetProject {
    /// Project name
    pub name: Option<String>,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<OutputFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects delete <NAME> [OPTIONS]")]
pub struct DeleteProject {
    /// Project name
    pub name: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "projects open <NAME> [OPTIONS]")]
pub struct OpenProject {
    /// Project name
    pub name: Option<String>,
}
