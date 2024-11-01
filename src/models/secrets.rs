use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

use owo_colors::OwoColorize;
use tabled::Tabled;

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct SecretWithoutDescription {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Value")]
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SecretOnlyName {
    #[tabled(rename = "Name")]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    pub name: String,
    pub value: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretOptional {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

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
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Value")]
    pub value: String,

    #[tabled(rename = "Description")]
    pub description: String,
}

impl From<String> for SecretOnlyName {
    fn from(name: String) -> Self {
        Self { name }
    }
}

impl From<Secret> for SecretWithDescription {
    fn from(secret: Secret) -> Self {
        Self {
            name: secret.name,
            value: secret.value,
            description: secret.description.unwrap_or("".to_string()),
        }
    }
}

impl From<Secret> for SecretWithoutDescription {
    fn from(secret: Secret) -> Self {
        Self {
            name: secret.name,
            value: secret.value,
        }
    }
}

impl Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // writeln!(f, "{} {}", "Key:".green(), self.key)?;
        // writeln!(f, "{} {}", "Value:".green(), self.value)?;
        write!(f, "{} {}", format!("{}:", self.name).green(), self.value)?;

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
pub struct UpdateSecretDescriptionPayload {
    pub description: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenamedSecret {
    pub name: String,
    pub new_name: String,
}

impl RenamedSecret {
    pub fn get_name(&self) -> String {
        self.name.to_string()
    }
}

pub type RenameSecretsPayload = Vec<RenamedSecret>;

// response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSecretsResponse {
    pub not_found_secrets: Vec<String>,
    pub deleted_count: usize,
}

// response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSecretsResponse {
    pub not_found_secrets: Vec<String>,
    pub updated_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAllSecretsResponse {
    pub deleted_count: usize,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretsSearchOutputFormat {
    #[default]
    List,
    Table,
    Json,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectSecretSearchedByName {
    #[serde(rename = "secretValue")]
    pub value: Option<String>,
    pub environments: Vec<SecretsSearchEnvironment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretsSearchEnvironment {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSecretSearchedByNameTable {
    #[tabled(rename = "Value")]
    pub secret_value: String,

    #[tabled(rename = "Environments")]
    pub environment_name: String,
}

impl From<ProjectSecretSearchedByName> for ProjectSecretSearchedByNameTable {
    fn from(secret: ProjectSecretSearchedByName) -> Self {
        let environment_names = secret
            .environments
            .iter()
            .map(|env| env.name.clone())
            .collect::<Vec<_>>()
            .join(", ");

        Self {
            secret_value: secret.value.unwrap_or("••••••••".to_string()),
            environment_name: environment_names,
        }
    }
}

impl Display for ProjectSecretSearchedByName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let valur_str = match &self.value {
            Some(value) => format!("{}", value),
            None => "••••••••".to_string(),
        };

        let environment_names = self
            .environments
            .iter()
            .map(|env| env.name.to_string())
            .collect::<Vec<_>>();

        writeln!(f, "Secret value: {}", valur_str)?;
        writeln!(f, "Environments: {}", environment_names.join(", "))?;

        Ok(())
    }
}
