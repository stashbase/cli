use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::human_datetime::get_human_datetime;

use super::secrets::Secret;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,

    // date string
    pub created_at: String,
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub locked: bool,
    pub is_production: bool,
    pub secret_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct TableEnvironment {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(display_with = "display_option")]
    #[tabled(rename = "Description", order = 6)]
    pub description: Option<String>,

    #[tabled(rename = "Locked", order = 4)]
    pub locked: bool,

    #[tabled(rename = "Production", order = 3)]
    pub is_production: bool,

    #[tabled(rename = "Secrets", order = 5)]
    pub secret_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct TableEnvironmentWithoutDescription {
    #[tabled(rename = "Id", order = 0)]
    pub id: String,

    #[tabled(rename = "Name", order = 1)]
    pub name: String,

    // date string
    #[tabled(rename = "Created at", order = 2)]
    pub created_at: String,

    #[tabled(rename = "Locked", order = 4)]
    pub locked: bool,

    #[tabled(rename = "Production", order = 3)]
    pub is_production: bool,

    #[tabled(rename = "Secrets", order = 5)]
    pub secret_count: usize,
}

impl From<Environment> for TableEnvironment {
    fn from(env: Environment) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        Self {
            id: env.id,
            created_at,
            name: env.name,
            description: env.description,
            locked: env.locked,
            is_production: env.is_production,
            secret_count: env.secret_count,
        }
    }
}

impl From<Environment> for TableEnvironmentWithoutDescription {
    fn from(env: Environment) -> Self {
        let (formatted, relative) = get_human_datetime(&env.created_at);
        let created_at = format!("{} ({})", formatted, relative);

        Self {
            id: env.id,
            created_at,
            name: env.name,
            locked: env.locked,
            is_production: env.is_production,
            secret_count: env.secret_count,
        }
    }
}

fn display_option(d: &Option<String>) -> String {
    match d {
        Some(s) => format!("{}", s),
        None => format!(""),
    }
}

impl Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;

        writeln!(f, "{} {}", "Id:".green(), self.id)?;
        writeln!(f, "{} {}", "Name:".green(), self.name)?;
        writeln!(f, "{} {}", "Locked:".green(), self.locked)?;
        writeln!(f, "{} {}", "Production:".green(), self.is_production)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

        writeln!(f, "{} {}", "Secret count:".green(), self.secret_count)?;

        Ok(())
    }
}

// requests

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatEnvironmentPayload {
    pub name: String,
    pub description: Option<String>,
    pub is_production: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<Secret>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEnvironmentResponse {
    pub id: String,
    pub name: String,

    #[serde(rename = "dashboardUrl")]
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

// load
#[derive(Debug, Serialize)]
pub struct LoadEnvironmentPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
}

impl LoadEnvironmentPayload {
    pub fn new(only: Option<Vec<String>>, exclude: Option<Vec<String>>) -> Self {
        Self { only, exclude }
    }
}

// NOTE: compare
#[derive(Debug, Serialize)]
pub struct CompareEnvironmentsPayload {
    pub name: String,
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
