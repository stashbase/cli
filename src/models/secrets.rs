use clap::ValueEnum;
use linked_hash_map::LinkedHashMap;
use linked_hash_set::LinkedHashSet;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use owo_colors::OwoColorize;
use tabled::Tabled;

use crate::{cmd::config::SecretsOutputFormat, utils};

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

        if let Some(description) = &self.description {
            writeln!(f, "{}", description.blue())?;
            writeln!(f, "{}", "-".repeat(self.name.len()).blue())?;

            // writeln!(f, "{} {}", "- description:".blue(), description)?;
        }

        write!(f, "{} {}", format!("{}:", self.name).green(), self.value)?;

        // if self.description.is_some() {
        //     writeln!(f, "")?;
        // }

        Ok(())
    }
}

pub trait ValidateSecrets {
    fn validate(&self) -> anyhow::Result<()>;
    fn get_reference_warnings(&self) -> SecretReferenceWarnings;
}

pub trait FormatSecrets {
    fn format(&mut self);
}

impl ValidateSecrets for Vec<Secret> {
    fn validate(&self) -> anyhow::Result<()> {
        utils::validation::validate_secrets(self)
    }

    fn get_reference_warnings(&self) -> SecretReferenceWarnings {
        utils::validation::get_secrets_reference_warnings(self)
    }
}

impl FormatSecrets for Vec<Secret> {
    fn format(&mut self) {
        utils::validation::format_secrets_input(self);
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

// Search secrets
#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretsSearchOutputFormat {
    #[default]
    List,
    Table,
    Yaml,
    Json,
}

impl From<SecretsOutputFormat> for Option<SecretsSearchOutputFormat> {
    fn from(format: SecretsOutputFormat) -> Self {
        match format {
            SecretsOutputFormat::Table => Some(SecretsSearchOutputFormat::Table),
            SecretsOutputFormat::Json => Some(SecretsSearchOutputFormat::Json),
            SecretsOutputFormat::List => Some(SecretsSearchOutputFormat::List),
            SecretsOutputFormat::Yaml => Some(SecretsSearchOutputFormat::Yaml),
            SecretsOutputFormat::Dotenv => None,
        }
    }
}

pub type SecretSearchedValue = Option<String>;

pub trait SecretValueDisplay {
    fn display(&self) -> String;
}

impl SecretValueDisplay for SecretSearchedValue {
    fn display(&self) -> String {
        match self {
            Some(value) => value.to_string(),
            None => "••••••••".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectSecretSearchedByName {
    #[serde(rename = "secretValue")]
    pub value: SecretSearchedValue,
    pub environments: Vec<SecretsSearchEnvironment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretsSearchEnvironment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
}

impl SecretsSearchEnvironment {
    pub fn get_name_string(&self) -> String {
        self.name.to_string()
    }

    pub fn get_id_string(&self) -> Option<String> {
        self.id.clone()
    }

    pub fn get_name_id_string(&self) -> String {
        match &self.id {
            Some(id) => format!("{} ({})", self.name, id),
            None => self.name.to_string(),
        }
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
            .map(|env| env.id.clone().unwrap_or("".to_string()))
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
pub struct ProjectSecretSearchedByNameTable {
    #[tabled(rename = "Value")]
    pub secret_value: String,

    #[tabled(rename = "Environments")]
    pub environments: String,
}

impl From<ProjectSecretSearchedByName> for ProjectSecretSearchedByNameTable {
    fn from(secret: ProjectSecretSearchedByName) -> Self {
        let environments_str = secret.environments.get_names_ids_string();

        Self {
            secret_value: secret.value.display(),
            environments: environments_str,
        }
    }
}

impl Display for ProjectSecretSearchedByName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let environments_str = self.environments.get_names_ids_string();

        writeln!(f, "Secret value: {}", self.value.display())?;
        writeln!(f, "Environments: {}", environments_str)?;

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
pub struct ProjectSecretSearchedByValueTable {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Environments")]
    pub environments: String,
}

impl From<ProjectSecretSearchedByValue> for ProjectSecretSearchedByValueTable {
    fn from(secret: ProjectSecretSearchedByValue) -> Self {
        let environments_str = secret.environments.get_names_ids_string();

        Self {
            name: secret.name,
            environments: environments_str,
        }
    }
}

impl Display for ProjectSecretSearchedByValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let environments_str = self.environments.get_names_ids_string();

        writeln!(f, "Secret name: {}", self.name)?;
        writeln!(f, "Environments: {}", environments_str)?;

        Ok(())
    }
}

// worksapce search secrets
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSecretSearchedByName {
    #[serde(rename = "secretValue")]
    pub value: SecretSearchedValue,
    pub project: WorkspaceSecretSearchProject,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSecretSearchProject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub environments: Vec<SecretsSearchEnvironment>,
}

impl WorkspaceSecretSearchProject {
    pub fn get_name_id_string(&self) -> String {
        match &self.id {
            Some(id) => format!("{} ({})", self.name, id),
            None => self.name.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct WorkspaceSecretSearchedByNameTable {
    #[tabled(rename = "Value")]
    pub secret_value: String,

    #[tabled(rename = "Project")]
    pub project: String,

    #[tabled(rename = "Environments")]
    pub environments: String,
}

impl Display for WorkspaceSecretSearchedByName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let environments_str = self.project.environments.get_names_ids_string();

        writeln!(f, "Secret value: {}", self.value.display())?;
        writeln!(f, "Project: {}", self.project.get_name_id_string())?;
        writeln!(f, "Environments: {}", environments_str)?;

        Ok(())
    }
}

impl From<WorkspaceSecretSearchedByName> for WorkspaceSecretSearchedByNameTable {
    fn from(secret: WorkspaceSecretSearchedByName) -> Self {
        let value_str = secret.value.display();
        let project_str = secret.project.get_name_id_string();
        let environments_str = secret.project.environments.get_names_ids_string();

        Self {
            project: project_str,
            secret_value: value_str,
            environments: environments_str,
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
        let project_str = self.project.get_name_id_string();
        let environments_str = self.project.environments.get_names_ids_string();

        writeln!(f, "Secret name: {}", self.name)?;
        writeln!(f, "Project: {}", project_str)?;
        writeln!(f, "Environments: {}", environments_str)?;

        Ok(())
    }
}

// workspace search secrets by value table
#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct WorkspaceSecretSearchedByValueTable {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Project")]
    pub project: String,

    #[tabled(rename = "Environments")]
    pub environments: String,
}

impl From<WorkspaceSecretSearchedByValue> for WorkspaceSecretSearchedByValueTable {
    fn from(secret: WorkspaceSecretSearchedByValue) -> Self {
        let project_str = secret.project.get_name_id_string();
        let environments_str = secret.project.environments.get_names_ids_string();

        Self {
            name: secret.name,
            project: project_str,
            environments: environments_str,
        }
    }
}

pub type InvalidFormatReferences = LinkedHashMap<String, Vec<String>>;
pub type NotFoundReferences = InvalidFormatReferences;

pub struct SecretReferenceWarnings {
    pub invalid_format: InvalidFormatReferences,
    // NOTE: refering secrets that do not exist (within input)
    // (names, reference)
    pub not_found: NotFoundReferences,
    // name of secrets that have empty references to other secrets (counts also whitespace)
    pub empty_value: LinkedHashSet<String>,
}

impl SecretReferenceWarnings {
    pub fn new() -> Self {
        Self {
            invalid_format: LinkedHashMap::new(),
            not_found: NotFoundReferences::new(),
            empty_value: LinkedHashSet::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.invalid_format.len() == 0 && self.not_found.len() == 0 && self.empty_value.len() == 0
    }
}

impl std::fmt::Display for SecretReferenceWarnings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.invalid_format.is_empty() {
            let hint_str = self
                .invalid_format
                .iter()
                .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;

            writeln!(f, "- message: invalid secret references format")?;
            writeln!(f, "- secrets: {} \n", hint_str)?;
        }

        if !self.not_found.is_empty() {
            let hint_str = self
                .not_found
                .iter()
                .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;

            writeln!(
                f,
                "- message: references to non-existent secrets (within input)"
            )?;
            writeln!(f, "- secrets: {} \n", hint_str)?;
        }

        if !self.empty_value.is_empty() {
            let hint_str = self
                .empty_value
                .iter()
                .map(|s| format!("{}", s))
                .collect::<Vec<_>>()
                .join(", ");

            writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;

            writeln!(f, "- message: empty references to other secrets")?;
            writeln!(f, "- secrets: {} \n", hint_str)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ReferencesValidation {
    pub self_referenced_secrets: Vec<String>, // vec of secrets (names)
    pub invalid_format_references: InvalidFormatReferences,
}

impl ReferencesValidation {
    pub fn new(
        self_referenced_secrets: Option<HashSet<String>>,
        invalid_format_references: Option<InvalidFormatReferences>,
    ) -> Self {
        Self {
            self_referenced_secrets: match self_referenced_secrets {
                None => Vec::new(),
                Some(r) => r.into_iter().collect(),
            },
            invalid_format_references: match invalid_format_references {
                None => LinkedHashMap::new(),
                Some(r) => r,
            },
        }
    }
    pub fn is_empty(&self) -> bool {
        self.invalid_format_references.len() == 0 && self.self_referenced_secrets.len() == 0
    }
}

#[derive(Debug)]
pub struct ReferencesValidationWithExistence {
    pub self_referenced_secrets: Vec<String>, // vec of secrets (names)
    pub invalid_format: InvalidFormatReferences,
    // NOTE: refering secrets that do not exist (within input)
    // (names, reference)
    pub not_found: NotFoundReferences,
}

impl ReferencesValidationWithExistence {
    pub fn new() -> Self {
        Self {
            self_referenced_secrets: Vec::new(),
            invalid_format: LinkedHashMap::new(),
            not_found: NotFoundReferences::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.invalid_format.len() == 0
            && self.self_referenced_secrets.len() == 0
            && self.not_found.len() == 0
    }
}
