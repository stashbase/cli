use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

use crate::utils::human_datetime::get_human_datetime;

#[derive(Debug, Serialize, Deserialize)]
pub enum EnvironmentType {
    DEVELOPMENT,
    TESTING,
    STAGING,
    PRODUCTION,
}

impl Display for EnvironmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvironmentType::DEVELOPMENT => write!(f, "Development"),
            EnvironmentType::TESTING => write!(f, "Testing"),
            EnvironmentType::STAGING => write!(f, "Staging"),
            EnvironmentType::PRODUCTION => write!(f, "Production"),
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
    pub env_type: EnvironmentType,
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
