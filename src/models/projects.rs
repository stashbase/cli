use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::{human_datetime::get_human_datetime, output::ColorizeIfTerminal};

use super::shared::PaginationMetadata;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    // date string
    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProjectPayload {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProjectPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProjectResponse {
    pub id: String,

    #[serde(skip_serializing)]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SingleListProject {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    // only for personal auth (api key)
    #[tabled(display_with = "display_bool_option")]
    #[tabled(rename = "Full access", order = 3)]
    pub full_access: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 5)]
    pub description: Option<String>,

    #[tabled(rename = "Environments", order = 4)]
    pub environment_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SingleListProjectWithoutDescription {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    // only for personal auth (api key)
    #[tabled(display_with = "display_bool_option")]
    #[tabled(rename = "Full access", order = 3)]
    pub full_access: Option<bool>,

    #[tabled(rename = "Environments", order = 4)]
    pub environment_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectList {
    pub data: Vec<SingleListProject>,
    pub pagination: PaginationMetadata,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SingleProjectTable {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,
    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Full access", order = 3)]
    // only for personal auth (api key)
    pub full_access: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 5)]
    pub description: Option<String>,

    #[tabled(rename = "Environments", order = 4)]
    pub environment_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleProject {
    pub id: String,

    pub name: String,
    // date string
    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub environment_count: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    // only for personal auth (api key)
    pub full_access: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SingleProjectWithCountNoDescriptionTable {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Full access", order = 3)]
    // only for personal auth (api key)
    pub full_access: Option<String>,

    #[tabled(rename = "Environments", order = 4)]
    pub environment_count: usize,
}

// TODO: rename roles?
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProjectUserRole {
    Viewer,
    Editor,
    Admin,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWithCountNoDescriptionTable {
    #[tabled(rename = "Name", order = 0)]
    pub name: String,
    // date string
    #[tabled(rename = "Created at", order = 1)]
    pub created_at: String,

    #[tabled(rename = "Environments", order = 2)]
    pub environment_count: usize,
}

fn display_option(d: &Option<String>) -> String {
    match d {
        Some(s) => format!("{}", s),
        None => format!("---"),
    }
}

fn display_bool_option(b: &Option<bool>) -> String {
    match b {
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
        None => "---".to_string(),
    }
}

impl From<SingleListProject> for SingleListProjectWithoutDescription {
    fn from(project: SingleListProject) -> Self {
        Self {
            id: project.id,
            name: project.name,
            created_at: project.created_at,
            full_access: project.full_access,
            environment_count: project.environment_count,
        }
    }
}

impl Display for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "Project name:".green_if_tty(), self.name)?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(
            f,
            "{} {} ({})",
            "Created at:".green_if_tty(),
            formatted,
            relative
        )?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green_if_tty(), description)?;
        }

        Ok(())
    }
}

impl Display for SingleListProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(
            f,
            "{} {} ({})",
            "Created at:".green_if_tty(),
            formatted,
            relative
        )?;

        writeln!(f, "{} {}", "Id".green_if_tty(), self.id)?;
        writeln!(f, "{} {}", "Project name:".green_if_tty(), self.name)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green_if_tty(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".green_if_tty(),
            self.environment_count
        )?;

        Ok(())
    }
}

impl Display for ProjectUserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectUserRole::Viewer => write!(f, "{}", "Viewer"),
            ProjectUserRole::Editor => write!(f, "{}", "Editor"),
            ProjectUserRole::Admin => write!(f, "{}", "Admin"),
        }
    }
}

impl Display for SingleProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(
            f,
            "{} {} ({})",
            "Created at:".green_if_tty(),
            formatted,
            relative
        )?;
        writeln!(f, "{} {}", "Id:".green_if_tty(), self.id)?;
        writeln!(f, "{} {}", "Name:".green_if_tty(), self.name)?;

        if let Some(full_access) = &self.full_access {
            writeln!(f, "{} {}", "Full access:".green_if_tty(), full_access)?;
        }

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green_if_tty(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".green_if_tty(),
            self.environment_count
        )?;

        Ok(())
    }
}

impl From<SingleProject> for SingleProjectTable {
    fn from(project: SingleProject) -> Self {
        let (formatted, relative) = get_human_datetime(&project.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let full_access = match project.full_access {
            Some(access) => Some(format!("{}", access)),
            None => None,
        };

        Self {
            created_at,
            id: project.id,
            name: project.name,
            full_access,
            description: project.description,
            environment_count: project.environment_count,
        }
    }
}

impl From<SingleProject> for SingleProjectWithCountNoDescriptionTable {
    fn from(project: SingleProject) -> Self {
        let (formatted, relative) = get_human_datetime(&project.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let full_access = match project.full_access {
            Some(access) => Some(format!("{}", access)),
            None => Some("false".to_string()),
        };

        Self {
            created_at,
            id: project.id,
            name: project.name,
            full_access,
            environment_count: project.environment_count,
        }
    }
}
