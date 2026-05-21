use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::{
    human_datetime::get_human_datetime,
    output::{write_indented, ColorizeIfColoredOutput},
};

use super::secrets::Secret;

#[derive(Debug, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,

    // date string
    pub created_at: String,
    pub updated_at: String,
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub is_production: bool,
    pub secret_count: usize,

    // only for personal auth (api key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_role: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<EnvironmentProjectReference>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentProjectReference {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct TableEnvironment {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    // only for personal auth (api key)
    #[tabled(rename = "User role", order = 3)]
    pub user_role: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 6)]
    pub description: Option<String>,

    #[tabled(rename = "Production", order = 4)]
    pub is_production: bool,

    #[tabled(rename = "Secrets", order = 5)]
    pub secret_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct TableEnvironmentWithoutDescription {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    // only for personal auth (api key)
    #[tabled(rename = "User role", order = 3)]
    pub user_role: String,

    #[tabled(rename = "Production", order = 4)]
    pub is_production: bool,

    #[tabled(rename = "Secrets", order = 5)]
    pub secret_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct TableEnvironmentWithProject {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    // only for personal auth (api key)
    #[tabled(rename = "User role", order = 3)]
    pub user_role: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 6)]
    pub description: Option<String>,

    #[tabled(rename = "Production", order = 4)]
    pub is_production: bool,

    #[tabled(rename = "Secrets", order = 5)]
    pub secret_count: usize,

    #[tabled(rename = "Project", order = 7)]
    pub project: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct TableEnvironmentWithProjectWithoutDescription {
    #[tabled(rename = "ID", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    // only for personal auth (api key)
    #[tabled(rename = "User role", order = 3)]
    pub user_role: String,

    #[tabled(rename = "Production", order = 4)]
    pub is_production: bool,

    #[tabled(rename = "Secrets", order = 5)]
    pub secret_count: usize,

    #[tabled(rename = "Project", order = 6)]
    pub project: String,
}

impl From<Environment> for TableEnvironment {
    fn from(env: Environment) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let user_role = match env.user_role {
            Some(role) => role.to_string(),
            None => "---".to_string(),
        };

        Self {
            id: env.id,
            created_at,
            name: env.name,
            description: env.description,
            user_role,
            is_production: env.is_production,
            secret_count: env.secret_count,
        }
    }
}

impl From<Environment> for TableEnvironmentWithoutDescription {
    fn from(env: Environment) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let user_role = match env.user_role {
            Some(role) => role.to_string(),
            None => "---".to_string(),
        };

        Self {
            id: env.id,
            created_at,
            name: env.name,
            user_role,
            is_production: env.is_production,
            secret_count: env.secret_count,
        }
    }
}

impl From<Environment> for TableEnvironmentWithProject {
    fn from(env: Environment) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let user_role = match env.user_role {
            Some(role) => role.to_string(),
            None => "---".to_string(),
        };

        let project = match &env.project {
            Some(project) => {
                format!("{} ({})", project.name, project.id)
            }
            None => "---".to_string(),
        };

        Self {
            id: env.id,
            created_at,
            name: env.name,
            description: env.description,
            user_role,
            is_production: env.is_production,
            secret_count: env.secret_count,
            project,
        }
    }
}

impl From<Environment> for TableEnvironmentWithProjectWithoutDescription {
    fn from(env: Environment) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        let user_role = match env.user_role {
            Some(role) => role.to_string(),
            None => "---".to_string(),
        };

        let project = match &env.project {
            Some(project) => {
                format!("{} ({})", project.name, project.id)
            }
            None => "---".to_string(),
        };

        Self {
            id: env.id,
            created_at,
            name: env.name,
            user_role,
            is_production: env.is_production,
            secret_count: env.secret_count,
            project,
        }
    }
}

fn display_option(d: &Option<String>) -> String {
    match d {
        Some(s) => format!("{}", s),
        None => format!("---"),
    }
}

impl Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

        writeln!(f, "{} {}", "ID:".blue_bold_if_tty(), self.id)?;
        writeln!(f, "{} {}", "Name:".blue_bold_if_tty(), self.name)?;
        writeln!(
            f,
            "{} {}",
            "Production:".blue_bold_if_tty(),
            self.is_production
        )?;

        if let Some(user_role) = &self.user_role {
            writeln!(f, "{} {}", "User role:".blue_bold_if_tty(), user_role)?;
        }

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".blue_bold_if_tty(), description)?;
        }

        writeln!(
            f,
            "{} {}",
            "Secret count:".blue_bold_if_tty(),
            self.secret_count
        )?;

        if let Some(project) = &self.project {
            writeln!(f, "{}", "Project:".blue_bold_if_tty(),)?;

            write_indented(
                f,
                2,
                &format!("{} {}", "ID:".blue_bold_if_tty(), project.id),
            )?;
            write_indented(
                f,
                2,
                &format!("{} {}", "Name:".blue_bold_if_tty(), project.name),
            )?;
        }

        Ok(())
    }
}

// requests

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatEnvironmentPayload {
    pub name: String,
    pub description: Option<String>,
    pub is_production: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<Secret>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateEnvironmentResponse {
    pub id: String,
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateEnvironmentPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_production: Option<bool>,
}

pub type CompareEnvironmentsResponse = Vec<CompareEnvironmentsItem>;

#[derive(Debug, Serialize, Deserialize)]
pub struct CompareEnvironmentsItem {
    pub name: String,
    //
    pub values: Vec<Option<String>>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct CompareEnvironmentsTableItem {
    #[tabled(rename = "Secret name", order = 0)]
    pub name: String,

    #[tabled(rename = "Value 1", order = 0)]
    pub value_1: String,

    #[tabled(rename = "Value 2", order = 0)]
    pub value_2: String,
}

impl From<CompareEnvironmentsItem> for CompareEnvironmentsTableItem {
    fn from(item: CompareEnvironmentsItem) -> Self {
        Self {
            name: item.name,
            value_1: item
                .values
                .get(0)
                .cloned()
                .unwrap()
                .unwrap_or_else(|| "".to_string()),
            value_2: item
                .values
                .get(1)
                .cloned()
                .unwrap()
                .unwrap_or_else(|| "".to_string()),
        }
    }
}
