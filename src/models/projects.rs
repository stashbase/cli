use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

use crate::utils::human_datetime::get_human_datetime;

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
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWithCount {
    pub name: String,
    // date string
    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub environment_count: usize,
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

impl Display for ProjectWithCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "Project name:".green(), self.name)?;

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(f, "{} {} ({})", "Created at:".green(), formatted, relative)?;

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
