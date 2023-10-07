use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    pub key: String,
    pub value: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "Key:".green(), self.key)?;
        writeln!(f, "{} {}", "Value:".green(), self.value)?;

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "Description:".green(), description)?;
        }

        Ok(())
    }
}
