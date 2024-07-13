use serde::{Deserialize, Serialize};
use std::fmt::Display;

use owo_colors::OwoColorize;
use tabled::Tabled;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SecretWithoutDescription {
    #[tabled(rename = "Key")]
    pub key: String,

    #[tabled(rename = "Value")]
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SecretOnlyKey {
    #[tabled(rename = "Key")]
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    pub key: String,
    pub value: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Secret {
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SecretWithDescription {
    #[tabled(rename = "Key")]
    pub key: String,

    #[tabled(rename = "Value")]
    pub value: String,

    #[tabled(rename = "Description")]
    pub description: String,
}

impl From<String> for SecretOnlyKey {
    fn from(key: String) -> Self {
        Self { key }
    }
}

impl From<Secret> for SecretWithDescription {
    fn from(secret: Secret) -> Self {
        Self {
            key: secret.key,
            value: secret.value,
            description: secret.description.unwrap_or("".to_string()),
        }
    }
}

impl From<Secret> for SecretWithoutDescription {
    fn from(secret: Secret) -> Self {
        Self {
            key: secret.key,
            value: secret.value,
        }
    }
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
#[serde(rename_all = "camelCase")]
pub struct GetSelectedSecretsPayload {
    pub keys: Vec<String>,
    pub replace_refs: bool,
}

#[derive(Debug, Serialize)]
pub struct DeleteSecretsPayload {
    pub keys: Vec<String>,
}

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
