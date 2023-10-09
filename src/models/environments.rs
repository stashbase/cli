use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

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
            EnvType::DEVELOPMENT => write!(f, "Development"),
            EnvType::TESTING => write!(f, "Testing"),
            EnvType::STAGING => write!(f, "Staging"),
            EnvType::PRODUCTION => write!(f, "Production"),
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
    // date string
    pub created_at: String,
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub locked: bool,

    #[serde(rename = "type")]
    pub env_type: EnvType,
}

impl Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "Env name:".green(), self.name)?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;
        writeln!(f, "{} {}", "Type:".green(), self.env_type)?;
        writeln!(f, "{} {}", "Locked:".green(), self.locked)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

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
