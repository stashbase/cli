use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::{human_datetime::get_human_datetime, output::ColorizeIfColoredOutput};

use super::shared::PaginationMetadata;

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,

    pub name: String,
    // date string
    pub created_at: String,
    pub updated_at: String,

    pub description: Option<String>,

    #[serde(default)]
    pub environment_count: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_access: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard_url: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SingleListProject {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    // only for personal auth (api key)
    #[tabled(display_with = "display_bool_option")]
    #[tabled(rename = "Full access", order = 2)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_access: Option<bool>,

    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 4)]
    pub description: Option<String>,

    #[tabled(rename = "Environments", order = 3)]
    pub environment_count: usize,

    #[tabled(rename = "Created", order = 5)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 6)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SingleListProjectWithoutDescription {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    // only for personal auth (api key)
    #[tabled(display_with = "display_bool_option")]
    #[tabled(rename = "Full access", order = 2)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_access: Option<bool>,

    #[tabled(rename = "Environments", order = 3)]
    pub environment_count: usize,

    #[tabled(rename = "Created", order = 4)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 5)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectList {
    pub data: Vec<SingleListProject>,
    pub pagination: PaginationMetadata,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SingleProjectTable {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Full access", order = 2)]
    // only for personal auth (api key)
    pub full_access: Option<String>,

    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 4)]
    pub description: Option<String>,

    #[tabled(rename = "Environments", order = 3)]
    pub environment_count: usize,

    // date string
    #[tabled(rename = "Created", order = 5)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 6)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SingleProject {
    pub id: String,

    pub name: String,
    // date string
    pub created_at: String,
    pub updated_at: String,

    pub description: Option<String>,

    pub environment_count: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    // only for personal auth (api key)
    pub full_access: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SingleProjectWithCountNoDescriptionTable {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Full access", order = 2)]
    // only for personal auth (api key)
    pub full_access: Option<String>,

    #[tabled(rename = "Environments", order = 3)]
    pub environment_count: usize,

    // date string
    #[tabled(rename = "Created", order = 4)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 5)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectUserRole {
    Viewer,
    Editor,
    Admin,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct ProjectWithCountNoDescriptionTable {
    #[tabled(rename = "Name", order = 0)]
    pub name: String,
    // date string
    #[tabled(rename = "Environments", order = 1)]
    pub environment_count: usize,

    #[tabled(rename = "Created", order = 2)]
    pub created_at: String,

    #[tabled(rename = "Updated", order = 3)]
    pub updated_at: String,
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
            updated_at: project.updated_at,
            full_access: project.full_access,
            environment_count: project.environment_count,
        }
    }
}

impl Display for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;
        writeln!(f, "{} {}", "Name:".blue_bold_if_tty(), self.name)?;

        match &self.description {
            Some(description) => {
                writeln!(f, "{} {}", "Description:".blue_bold_if_tty(), description)?;
            }
            None => {
                writeln!(f, "{} ---", "Description:".blue_bold_if_tty())?;
            }
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".blue_bold_if_tty(),
            self.environment_count
        )?;

        writeln!(
            f,
            "{} {}",
            "Full access:".blue_bold_if_tty(),
            display_bool_option(&self.full_access)
        )?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(
            f,
            "{} {} ({})",
            "Created at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        let (formatted, relative) = get_human_datetime(&self.updated_at);

        writeln!(
            f,
            "{} {} ({})",
            "Updated at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        Ok(())
    }
}

impl Display for SingleListProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;
        writeln!(f, "{} {}", "Name:".blue_bold_if_tty(), self.name)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".blue_bold_if_tty(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".blue_bold_if_tty(),
            self.environment_count
        )?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(
            f,
            "{} {} ({})",
            "Created at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        if self.created_at != self.updated_at {
            let (formatted_updated, relative_updated) = get_human_datetime(&self.updated_at);

            writeln!(
                f,
                "{} {} ({})",
                "Updated at:".blue_bold_if_tty(),
                formatted_updated,
                relative_updated
            )?;
        }

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
        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;
        writeln!(f, "{} {}", "Name:".blue_bold_if_tty(), self.name)?;

        if let Some(full_access) = &self.full_access {
            writeln!(f, "{} {}", "Full access:".blue_bold_if_tty(), full_access)?;
        }

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".blue_bold_if_tty(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".blue_bold_if_tty(),
            self.environment_count
        )?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(
            f,
            "{} {} ({})",
            "Created at:".blue_bold_if_tty(),
            formatted,
            relative
        )?;

        if self.created_at != self.updated_at {
            let (formatted_updated, relative_updated) = get_human_datetime(&self.updated_at);

            writeln!(
                f,
                "{} {} ({})",
                "Updated at:".blue_bold_if_tty(),
                formatted_updated,
                relative_updated
            )?;
        }

        Ok(())
    }
}

impl From<SingleProject> for SingleProjectTable {
    fn from(project: SingleProject) -> Self {
        let (formatted_created, _) = get_human_datetime(&project.created_at);
        let (formatted_updated, _) = get_human_datetime(&project.updated_at);

        let full_access = match project.full_access {
            Some(access) => Some(format!("{}", access)),
            None => None,
        };

        Self {
            created_at: formatted_created,
            updated_at: formatted_updated,
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
        let (formatted_created, _) = get_human_datetime(&project.created_at);
        let (formatted_updated, _) = get_human_datetime(&project.updated_at);

        let full_access = match project.full_access {
            Some(access) => Some(format!("{}", access)),
            None => Some("false".to_string()),
        };

        Self {
            created_at: formatted_created,
            updated_at: formatted_updated,
            id: project.id,
            name: project.name,
            full_access,
            environment_count: project.environment_count,
        }
    }
}
