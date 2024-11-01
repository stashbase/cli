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

impl SecretsSearchEnvironment {
    pub fn get_name_string(&self) -> String {
        self.name.to_string()
    }

    pub fn get_id_string(&self) -> String {
        self.id.to_string()
    }

    pub fn get_name_id_string(&self) -> String {
        format!("{} ({})", self.name, self.id)
    }
}

pub trait SecretsSearchEnvironmentVecExt {
    fn get_names_ids_string(&self) -> String;
    fn get_names_string(&self) -> String;
    fn get_ids_string(&self) -> String;
}

// Implement the trait for Vec<SecretsSearchEnvironment>
impl SecretsSearchEnvironmentVecExt for Vec<SecretsSearchEnvironment> {
    fn get_names_string(&self) -> String {
        self.iter()
            .map(|env| env.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn get_ids_string(&self) -> String {
        self.iter()
            .map(|env| env.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn get_names_ids_string(&self) -> String {
        self.iter()
            .map(|env| env.get_name_id_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectSecretSearchedByValue {
    #[serde(rename = "secretName")]
    pub name: String,
    pub environments: Vec<SecretsSearchEnvironment>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSecretSearchedByValueTable {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Environments")]
    pub environment_names: String,
}

impl From<ProjectSecretSearchedByValue> for ProjectSecretSearchedByValueTable {
    fn from(secret: ProjectSecretSearchedByValue) -> Self {
        let environment_names = secret
            .environments
            .iter()
            .map(|env| env.name.to_string())
            .collect::<Vec<_>>();

        Self {
            name: secret.name,
            environment_names: environment_names.join(", "),
        }
    }
}

impl Display for ProjectSecretSearchedByValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let environment_names = self
            .environments
            .iter()
            .map(|env| env.name.to_string())
            .collect::<Vec<_>>();

        writeln!(f, "Secret name: {}", self.name)?;
        writeln!(f, "Environments: {}", environment_names.join(", "))?;

        Ok(())
    }
}

// worksapce search secrets
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSecretSearchedByName {
    #[serde(rename = "secretValue")]
    pub value: Option<String>,
    pub project: WorkspaceSecretSearchProject,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSecretSearchProject {
    pub id: String,
    pub name: String,
    pub environments: Vec<SecretsSearchEnvironment>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSecretSearchedByNameTable {
    #[tabled(rename = "Value")]
    pub secret_value: String,

    #[tabled(rename = "Project")]
    pub project_name: String,

    #[tabled(rename = "Environments")]
    pub environment_names: String,
}

impl Display for WorkspaceSecretSearchedByName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let valur_str = match &self.value {
            Some(value) => format!("{}", value),
            None => "••••••••".to_string(),
        };

        let environments_str = self.project.environments.get_names_ids_string();

        writeln!(f, "Secret value: {}", valur_str)?;
        writeln!(f, "Project: {}", self.project.name)?;
        writeln!(f, "Environments: {}", environments_str)?;

        Ok(())
    }
}

impl From<WorkspaceSecretSearchedByName> for WorkspaceSecretSearchedByNameTable {
    fn from(secret: WorkspaceSecretSearchedByName) -> Self {
        let valur_str = match &secret.value {
            Some(value) => format!("{}", value),
            None => "••••••••".to_string(),
        };

        let environment_names = secret
            .project
            .environments
            .iter()
            .map(|env| env.name.to_string())
            .collect::<Vec<_>>();

        Self {
            secret_value: valur_str,
            project_name: secret.project.name,
            environment_names: environment_names.join(", "),
        }
    }
}

// workspace search secrets by value
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSecretSearchedByValue {
    #[serde(rename = "secretName")]
    pub name: String,
    pub project: WorkspaceSecretSearchProject,
}

impl Display for WorkspaceSecretSearchedByValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let environment_names = self
            .project
            .environments
            .iter()
            .map(|env| env.name.to_string())
            .collect::<Vec<_>>();

        writeln!(f, "Secret name: {}", self.name)?;
        writeln!(f, "Project: {}", self.project.name)?;
        writeln!(f, "Environments: {}", environment_names.join(", "))?;

        Ok(())
    }
}

// workspace search secrets by value table
#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct WorkspaceSecretSearchedByValueTable {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Project")]
    pub project_name: String,

    #[tabled(rename = "Environments")]
    pub environment_names: String,
}

impl From<WorkspaceSecretSearchedByValue> for WorkspaceSecretSearchedByValueTable {
    fn from(secret: WorkspaceSecretSearchedByValue) -> Self {
        let environment_names = secret
            .project
            .environments
            .iter()
            .map(|env| env.name.to_string())
            .collect::<Vec<_>>();

        Self {
            name: secret.name,
            project_name: secret.project.name,
            environment_names: environment_names.join(", "),
        }
    }
}
