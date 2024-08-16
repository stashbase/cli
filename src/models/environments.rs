use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::{cmd::environments::EnvironmentType, utils::human_datetime::get_human_datetime};

use super::secrets::Secret;

#[derive(Debug, Serialize, Deserialize)]
pub enum EnvType {
    DEVELOPMENT,
    TESTING,
    STAGING,
    PRODUCTION,
}

impl Display for EnvType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // EnvType::DEVELOPMENT => write!(f, "Development"),
            // EnvType::TESTING => write!(f, "Testing"),
            // EnvType::STAGING => write!(f, "Staging"),
            // EnvType::PRODUCTION => write!(f, "Production"),
            EnvType::DEVELOPMENT => write!(f, "{}", "Development".blue()),
            EnvType::TESTING => write!(f, "{}", "Testing".cyan()),
            EnvType::STAGING => write!(f, "{}", "Staging".green()),
            EnvType::PRODUCTION => write!(f, "{}", "Production".red()),
        }
    }
}

impl From<EnvironmentType> for EnvType {
    fn from(e: EnvironmentType) -> Self {
        match e {
            EnvironmentType::Development => EnvType::DEVELOPMENT,
            EnvironmentType::Testing => EnvType::TESTING,
            EnvironmentType::Staging => EnvType::STAGING,
            EnvironmentType::Production => EnvType::PRODUCTION,
        }
    }
}

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

    #[serde(rename = "type")]
    pub env_type: EnvType,

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

    #[serde(rename = "type")]
    #[tabled(rename = "Type", order = 3)]
    pub env_type: String,

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

    #[serde(rename = "type")]
    #[tabled(rename = "Type", order = 3)]
    pub env_type: String,

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
            env_type: match env.env_type {
                EnvType::DEVELOPMENT => "Development".to_string(),
                EnvType::TESTING => "Testing".to_string(),
                EnvType::STAGING => "Staging".to_string(),
                EnvType::PRODUCTION => "Production".to_string(),
            },
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
            env_type: match env.env_type {
                EnvType::DEVELOPMENT => "Development".to_string(),
                EnvType::TESTING => "Testing".to_string(),
                EnvType::STAGING => "Staging".to_string(),
                EnvType::PRODUCTION => "Production".to_string(),
            },
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
        writeln!(f, "{} {}", "Type:".green(), self.env_type)?;
        writeln!(f, "{} {}", "Locked:".green(), self.locked)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

        writeln!(f, "{} {}", "Secret count:".green(), self.secret_count)?;

        Ok(())
    }
}

// requests

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatEnvironmentPayload {
    pub name: String,
    pub description: Option<String>,

    #[serde(rename = "type")]
    pub env_type: EnvType,

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

#[derive(Debug, Serialize)]
pub struct UpdateEnvironmentTypePayload {
    #[serde(rename = "type")]
    pub env_type: EnvType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateEnvironmentPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateEnvironmentPayload {
    pub name: String,
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
    pub key: String,
    //
    pub values: Vec<Option<String>>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct CompareEnvironmentsTableItem {
    #[tabled(rename = "Secret key", order = 0)]
    pub key: String,

    #[tabled(order = 1)]
    pub value_1: String,

    #[tabled(order = 2)]
    pub value_2: String,
}

impl From<CompareEnvironmentsItem> for CompareEnvironmentsTableItem {
    fn from(item: CompareEnvironmentsItem) -> Self {
        Self {
            key: item.key,
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
