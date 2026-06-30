#![allow(dead_code)]

use clap::ValueEnum;
use linked_hash_map::LinkedHashMap;
use linked_hash_set::LinkedHashSet;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt::Display};

use owo_colors::OwoColorize;
use tabled::Tabled;

use crate::{
    cmd::config::SecretsOutputFormat,
    utils::{
        self,
        output::{is_color_enabled, write_indented, ColorizeIfColoredOutput},
    },
};

use super::validation::InputValidationError;

#[derive(Debug, PartialEq, ValueEnum, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrintSecrets {
    /// Print only secret names
    Name,
    /// Print secret names and masked values
    Masked,
    /// Print secret names and full values
    Full,
}

impl Display for PrintSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name => write!(f, "names"),
            Self::Masked => write!(f, "masked"),
            Self::Full => write!(f, "full"),
        }
    }
}

impl PrintSecrets {
    pub fn is_name(&self) -> bool {
        self == &PrintSecrets::Name
    }
    pub fn is_masked(&self) -> bool {
        *self == PrintSecrets::Masked
    }
    pub fn is_full(&self) -> bool {
        self == &PrintSecrets::Full
    }
}

#[derive(Debug, Serialize, Deserialize, Tabled, Clone)]
pub struct SecretWithoutComment {
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

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SecretOnlyNameWithComment {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Comment")]
    pub comment: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Secret {
    pub name: String,
    pub value: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl Secret {
    pub fn without_comment(self) -> Self {
        Self {
            name: self.name,
            value: self.value,
            comment: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecretOptional {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl Secret {
    pub fn has_comment(&self) -> bool {
        self.comment.is_some()
    }
}

impl SecretOptional {
    pub fn has_comment(&self) -> bool {
        self.comment.is_some()
    }
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SecretWithComment {
    #[tabled(rename = "Name")]
    pub name: String,

    #[tabled(rename = "Value")]
    pub value: String,

    #[tabled(rename = "Comment")]
    pub comment: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecretMetadata {
    pub name: String,
    pub comment: Option<String>,
    pub version: u32,
    pub has_value: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecretMetadataListResponse {
    pub secrets: Vec<SecretMetadata>,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SecretMetadataTable {
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Comment")]
    pub comment: String,
    #[tabled(rename = "Version")]
    pub version: u32,
    #[tabled(rename = "Has Value")]
    pub has_value: bool,
    #[tabled(rename = "Created")]
    pub created_at: String,
    #[tabled(rename = "Updated")]
    pub updated_at: String,
    #[tabled(rename = "Last Accessed")]
    pub last_accessed_at: String,
}

#[derive(Debug, Serialize, Deserialize, Tabled)]
pub struct SecretMetadataTableWithoutComment {
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Version")]
    pub version: u32,
    #[tabled(rename = "Has Value")]
    pub has_value: bool,
    #[tabled(rename = "Created")]
    pub created_at: String,
    #[tabled(rename = "Updated")]
    pub updated_at: String,
    #[tabled(rename = "Last Accessed")]
    pub last_accessed_at: String,
}

impl From<SecretMetadata> for SecretMetadataTable {
    fn from(value: SecretMetadata) -> Self {
        Self {
            name: value.name,
            comment: value.comment.unwrap_or_default(),
            version: value.version,
            has_value: value.has_value,
            created_at: value.created_at,
            updated_at: value.updated_at,
            last_accessed_at: value.last_accessed_at.unwrap_or_default(),
        }
    }
}

impl From<SecretMetadata> for SecretMetadataTableWithoutComment {
    fn from(value: SecretMetadata) -> Self {
        Self {
            name: value.name,
            version: value.version,
            has_value: value.has_value,
            created_at: value.created_at,
            updated_at: value.updated_at,
            last_accessed_at: value.last_accessed_at.unwrap_or_default(),
        }
    }
}

impl From<String> for SecretOnlyName {
    fn from(name: String) -> Self {
        Self { name }
    }
}

impl From<Secret> for SecretWithComment {
    fn from(secret: Secret) -> Self {
        Self {
            name: secret.name,
            value: secret.value,
            comment: secret.comment.unwrap_or("".to_string()),
        }
    }
}

impl From<Secret> for SecretWithoutComment {
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

        if let Some(comment) = &self.comment {
            let comment_str = format!("# {}", comment);
            writeln!(f, "{}", comment_str.bright_blue_if_tty())?;
        }

        write!(
            f,
            "{} {}",
            format!("{}:", self.name).blue_bold_if_tty(),
            self.value
        )?;

        // if self.comment.is_some() {
        //     writeln!(f, "")?;
        // }

        Ok(())
    }
}

impl Display for SecretOptional {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(comment) = &self.comment {
            let comment_str = format!("# {}", comment);
            writeln!(f, "{}", comment_str.bright_blue_if_tty())?;
        }

        match &self.value {
            Some(value) => write!(
                f,
                "{} {}",
                format!("{}:", self.name).blue_bold_if_tty(),
                value
            )?,
            None => write!(f, "{}", self.name.as_str().blue_bold_if_tty())?,
        }

        Ok(())
    }
}

pub trait ValidateSecrets {
    fn validate(&self) -> Result<(), InputValidationError>;
    fn get_reference_warnings(&self) -> SecretReferenceWarnings;
}

pub trait FormatSecrets {
    fn format(&mut self);
}

impl ValidateSecrets for Vec<Secret> {
    fn validate(&self) -> Result<(), InputValidationError> {
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
pub struct UpdatedSecret {
    // The name of the secret to update
    pub name: String,

    // The new name of the secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,

    // The new value of the secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    // The new comment of the secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

pub type UpdateSecretsPayload = Vec<UpdatedSecret>;

pub trait ValidateUpdateSecrets {
    fn validate(&self) -> Result<(), InputValidationError>;
}

impl ValidateUpdateSecrets for Vec<UpdatedSecret> {
    fn validate(&self) -> Result<(), InputValidationError> {
        utils::validation::validate_update_secrets(self)
    }
}

#[derive(Debug, Serialize)]
pub struct SecretPropertiesToUpdate {
    pub new_name: Option<String>,
    pub value: Option<String>,
    pub comment: Option<String>,
}

// response
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSecretsResponse {
    // The number of secrets successfully created
    pub created_count: usize,
    // An array of secret names that already exist and were not created
    pub existing_secrets: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSecretsResponse {
    pub updated_count: usize,
    pub not_found_secrets: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpsertSecretsResponse {
    pub created_secrets: Vec<String>,
    pub updated_secrets: Vec<String>,
}

// response
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteSecretsResponse {
    pub not_found_secrets: Vec<String>,
    pub deleted_count: usize,
}

// response
#[derive(Debug, Serialize, Deserialize)]
pub struct RenameSecretsResponse {
    pub not_found_secrets: Vec<String>,
    pub updated_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteAllSecretsResponse {
    pub deleted_count: usize,
}

// Search secrets
#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretsSearchOutputFormat {
    #[default]
    Plain,
    Table,
    Yaml,
    Json,
}

impl From<SecretsOutputFormat> for Option<SecretsSearchOutputFormat> {
    fn from(format: SecretsOutputFormat) -> Self {
        match format {
            SecretsOutputFormat::Table => Some(SecretsSearchOutputFormat::Table),
            SecretsOutputFormat::Json => Some(SecretsSearchOutputFormat::Json),
            SecretsOutputFormat::Plain => Some(SecretsSearchOutputFormat::Plain),
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
    #[serde(rename = "secret_value", skip_serializing_if = "Option::is_none")]
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

        writeln!(
            f,
            "{} {}",
            "Secret value:".blue_bold_if_tty(),
            self.value.display()
        )?;
        writeln!(
            f,
            "{} {}",
            "Environments:".blue_bold_if_tty(),
            environments_str
        )?;

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectSecretSearchedByValue {
    #[serde(rename = "secret_name")]
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

        writeln!(f, "{} {}", "Secret name:".blue_bold_if_tty(), self.name)?;
        writeln!(
            f,
            "{} {}",
            "Environments:".blue_bold_if_tty(),
            environments_str
        )?;

        Ok(())
    }
}

// worksapce search secrets
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceSecretSearchedByName {
    #[serde(rename = "secret_value", skip_serializing_if = "Option::is_none")]
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

        writeln!(
            f,
            "{} {}",
            "Secret value:".blue_bold_if_tty(),
            self.value.display()
        )?;
        writeln!(
            f,
            "{} {}",
            "Project:".blue_bold_if_tty(),
            self.project.get_name_id_string()
        )?;
        writeln!(
            f,
            "{} {}",
            "Environments:".blue_bold_if_tty(),
            environments_str
        )?;

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
    #[serde(rename = "secret_name")]
    pub name: String,
    pub project: WorkspaceSecretSearchProject,
}

impl Display for WorkspaceSecretSearchedByValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let project_str = self.project.get_name_id_string();
        let environments_str = self.project.environments.get_names_ids_string();

        writeln!(f, "{} {}", "Secret name:".blue_bold_if_tty(), self.name)?;
        writeln!(f, "{} {}", "Project:".blue_bold_if_tty(), project_str)?;
        writeln!(
            f,
            "{} {}",
            "Environments:".blue_bold_if_tty(),
            environments_str
        )?;

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
    // self references
    pub self_reference: LinkedHashSet<String>,
}

impl SecretReferenceWarnings {
    pub fn new() -> Self {
        Self {
            invalid_format: LinkedHashMap::new(),
            not_found: NotFoundReferences::new(),
            empty_value: LinkedHashSet::new(),
            self_reference: LinkedHashSet::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.invalid_format.len() == 0
            && self.not_found.len() == 0
            && self.empty_value.len() == 0
            && self.self_reference.len() == 0
    }
}

impl std::fmt::Display for SecretReferenceWarnings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.self_reference.is_empty() {
            let secrets_str = self
                .self_reference
                .iter()
                .map(|k| format!("\"{}\"", k))
                .collect::<Vec<_>>()
                .join(", ");

            if is_color_enabled(false) {
                writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;
            } else {
                writeln!(f, "{}", format!("{}", "Input warning"))?;
            }

            writeln!(f, "- message: secrets referencing itself (within input)")?;
            writeln!(f, "- secrets: {} \n", secrets_str)?;
        }

        if !self.invalid_format.is_empty() {
            let secrets_str = self
                .invalid_format
                .iter()
                .map(|(k, v)| format!("\"{}\" (\"{}\")", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            if is_color_enabled(false) {
                writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;
            } else {
                writeln!(f, "{}", format!("{}", "Input warning"))?;
            }

            writeln!(f, "- message: invalid secret references format")?;
            writeln!(f, "- secrets: {} \n", secrets_str)?;
        }

        if !self.not_found.is_empty() {
            let secrets_str = self
                .not_found
                .iter()
                .map(|(k, v)| format!("\"{}\" (\"{}\")", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            if is_color_enabled(false) {
                writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;
            } else {
                writeln!(f, "{}", format!("{}", "Input warning"))?;
            }

            writeln!(
                f,
                "- message: references to non-existent secrets (within input)"
            )?;
            writeln!(f, "- secrets: {} \n", secrets_str)?;
        }

        if !self.empty_value.is_empty() {
            let secrets_str = self
                .empty_value
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ");

            if is_color_enabled(false) {
                writeln!(f, "{}", format!("{}", "Input warning").yellow().bold())?;
            } else {
                writeln!(f, "{}", format!("{}", "Input warning"))?;
            }

            writeln!(f, "- message: empty references to other secrets")?;
            writeln!(f, "- secrets: {} \n", secrets_str)?;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretsDiff {
    pub added: Vec<SecretOptional>,
    pub missing: Vec<SecretOptional>,
    pub modified: Vec<SecretDiffModified>,
}

impl SecretsDiff {
    pub fn new(
        added: Vec<SecretOptional>,
        missing: Vec<SecretOptional>,
        modified: Vec<SecretDiffModified>,
    ) -> Self {
        Self {
            added,
            missing,
            modified,
        }
    }

    pub fn sort(&mut self) {
        self.added.sort_by(|a, b| a.name.cmp(&b.name));
        self.missing.sort_by(|a, b| a.name.cmp(&b.name));
        self.modified.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.missing.is_empty() && self.modified.is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretDiffModified {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<SecretDiffModifiedChange>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretDiffModifiedChange {
    pub local: SecretDiffModifiedChangeItem,
    pub remote: SecretDiffModifiedChangeItem,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretDiffModifiedChangeItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl Display for SecretsDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.added.len() > 0 {
            writeln!(f, "{} ({})", "Added:".blue_bold_if_tty(), self.added.len())?;

            for secret in &self.added {
                let mut str: String;

                if secret.value.is_some() {
                    if secret.value.as_ref().unwrap().contains("\n") {
                        str = format!("{}", format!("+ {}:", secret.name).green_if_tty(),);

                        for line in secret.value.as_ref().unwrap().lines() {
                            if line.is_empty() {
                                continue;
                            }

                            str += &format!("\n  {}", line);
                        }
                    } else {
                        str = format!(
                            "{} {}",
                            format!("+ {}:", secret.name).green_if_tty(),
                            secret.value.as_ref().unwrap()
                        );
                    }
                } else {
                    str = format!("+ {}", secret.name).green_if_tty();
                }
                write_indented(f, 2, &str)?;
            }
        }

        if self.missing.len() > 0 {
            writeln!(f, "")?;
            writeln!(
                f,
                "{} ({})",
                "Missing:".blue_bold_if_tty(),
                self.missing.len()
            )?;

            for secret in &self.missing {
                let mut str: String;

                if secret.value.is_some() {
                    if secret.value.as_ref().unwrap().contains("\n") {
                        str = format!("{}", format!("- {}:", secret.name).red_if_tty(),);

                        for line in secret.value.as_ref().unwrap().lines() {
                            if line.is_empty() {
                                continue;
                            }

                            str += &format!("\n  {}", line);
                        }
                    } else {
                        str = format!(
                            "{} {}",
                            format!("- {}:", secret.name).red_if_tty(),
                            secret.value.as_ref().unwrap()
                        );
                    }
                } else {
                    str = format!("- {}", secret.name).red_if_tty();
                }

                write_indented(f, 2, &str)?;
            }
        }

        if self.modified.len() > 0 {
            writeln!(f, "")?;
            writeln!(
                f,
                "{} ({})",
                "Modified:".blue_bold_if_tty(),
                self.modified.len()
            )?;

            for secret in &self.modified {
                let str = format!("~ {}", secret.name);
                write_indented(f, 2, &str.yellow_if_tty())?;

                if let Some(changes) = &secret.changes {
                    if (changes.local.value.is_some() || changes.remote.value.is_some())
                        || (changes.local.comment.is_some() || changes.remote.comment.is_some())
                    {
                        let local_comment = changes.local.comment.as_ref();
                        let remote_comment = changes.remote.comment.as_ref();

                        // Handle local value
                        if let Some(local_comment) = local_comment {
                            let str = format!("  # {}", local_comment);
                            write_indented(f, 6, &str.bright_blue_if_tty())?;
                        }

                        if let Some(local_value) = &changes.local.value {
                            if local_value.contains("\n") {
                                let mut local_str = String::from("  • Local:");
                                for line in local_value.lines() {
                                    if line.is_empty() {
                                        continue;
                                    }
                                    local_str += &format!("\n    {}", line);
                                }
                                write_indented(f, 4, &local_str)?;
                            } else {
                                let local_str = format!("  • Local: {}", local_value);
                                write_indented(f, 4, &local_str)?;
                            }
                        } else {
                            let local_str = format!("  • Local: {}", "••••••••");
                            write_indented(f, 4, &local_str)?;
                        }

                        // Handle remote value
                        if let Some(remote_comment) = remote_comment {
                            let str = format!("  # {}", remote_comment);
                            write_indented(f, 6, &str.bright_blue_if_tty())?;
                        }

                        if let Some(remote_value) = &changes.remote.value {
                            if remote_value.contains("\n") {
                                let mut remote_str = String::from("  • Remote:");
                                for line in remote_value.lines() {
                                    if line.is_empty() {
                                        continue;
                                    }
                                    remote_str += &format!("\n    {}", line);
                                }
                                write_indented(f, 4, &remote_str)?;
                            } else {
                                let remote_str = format!("  • Remote: {}", remote_value);
                                write_indented(f, 4, &remote_str)?;
                            }
                        } else {
                            let remote_str = format!("  • Remote: {}", "••••••••");
                            write_indented(f, 4, &remote_str)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
