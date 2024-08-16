use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::human_datetime::get_human_datetime;

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

    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 4)]
    pub description: Option<String>,

    #[tabled(rename = "Environments", order = 3)]
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

    #[tabled(rename = "Environments", order = 3)]
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
    #[tabled(rename = "User role", order = 3)]
    pub user_role: Option<String>,

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
    pub user_role: Option<ProjectUserRole>,
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
    #[tabled(rename = "User role", order = 3)]
    pub user_role: Option<String>,

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
        None => format!(""),
    }
}

// impl From<ProjectWithCount> for ProjectWithCountNoDescriptionTable {
//     fn from(project: ProjectWithCount) -> Self {
//         Self {
//             name: project.name,
//             created_at: project.created_at,
//             environment_count: project.environment_count,
//         }
//     }
// }
impl From<SingleListProject> for SingleListProjectWithoutDescription {
    fn from(project: SingleListProject) -> Self {
        Self {
            id: project.id,
            name: project.name,
            created_at: project.created_at,
            environment_count: project.environment_count,
        }
    }
}

impl Display for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "Project name:".green(), self.name)?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

        Ok(())
    }
}

impl Display for SingleListProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;

        writeln!(f, "{} {}", "Id".green(), self.id)?;
        writeln!(f, "{} {}", "Project name:".green(), self.name)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".green(),
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

        writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;
        writeln!(f, "{} {}", "Id:".green(), self.id)?;
        writeln!(f, "{} {}", "Name:".green(), self.name)?;

        if let Some(user_role) = &self.user_role {
            writeln!(f, "{} {}", "User role:".green(), user_role)?;
        }

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Environment count:".green(),
            self.environment_count
        )?;

        Ok(())
    }
}

impl From<SingleProject> for SingleProjectTable {
    fn from(env: SingleProject) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let user_role = match env.user_role {
            Some(role) => Some(format!("{}", role)),
            None => None,
        };

        Self {
            id: env.id,
            created_at,
            name: env.name,
            user_role,
            description: env.description,
            environment_count: env.environment_count,
        }
    }
}

impl From<SingleProject> for SingleProjectWithCountNoDescriptionTable {
    fn from(env: SingleProject) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let user_role = match env.user_role {
            Some(role) => Some(format!("{}", role)),
            None => None,
        };

        Self {
            id: env.id,
            created_at,
            user_role,
            name: env.name,
            environment_count: env.environment_count,
        }
    }
}
