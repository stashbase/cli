use serde::{Deserialize, Serialize};
use std::fmt::Display;

use owo_colors::OwoColorize;
use tabled::Tabled;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SecretWithoutDescription {
    pub key: String,
    pub value: String,
}

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
        // writeln!(f, "{} {}", "Key:".green(), self.key)?;
        // writeln!(f, "{} {}", "Value:".green(), self.value)?;
        write!(f, "{} {}", format!("{}:", self.key).green(), self.value)?;

        if self.description.is_some() {
            writeln!(f, "")?;
        }

        if let Some(description) = &self.description {
            writeln!(f, "{} {}", "- description:".blue(), description)?;
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct GetSelectedSecretsPayload {
    pub keys: Vec<String>,
}

pub type DeleteSecretsPayload = GetSelectedSecretsPayload;

#[derive(Debug, Serialize)]
pub struct UpdateSecretDescriptionPayload {
    pub description: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenamedSecret {
    pub key: String,
    pub new_key: String,
}

impl RenamedSecret {
    pub fn get_key(&self) -> String {
        self.key.to_string()
    }
}

pub type RenameSecretsPayload = Vec<RenamedSecret>;

// response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSecretsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_found: Vec<String>,
}

pub type RenameSecretsResponse = DeleteSecretsResponse;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAllSecretsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_empty: Option<bool>,
}
