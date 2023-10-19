use std::fmt::Display;

use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};

use crate::utils::human_datetime::get_human_datetime;

use super::environments::EnvType;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvChangelogList {
    // pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    pub pages: usize,

    pub data: Vec<EnvChangelogListItem>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvChangelogListItem {
    // short uuid
    pub id: String,
    pub created_at: String,
    pub user: Option<EnvChangelogUser>,
    pub change: EnvChangelogChange,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvChangelogUser {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum EnvChangelogChange {
    Change(EnvChangelogItemChange),
    SecretsChange(SecretsChange),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum SecretsChange {
    NoValues(SecretsChangeNoValues),
    WithValues(SecretsChangeWithValues),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretsChangeWithValues {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed: Option<Vec<RenamedSecretWithValue>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<Vec<SecretChangeKeyValue>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Vec<SecretChangeKeyValue>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<Vec<UpdatedSecretWithValues>>,
}

// with values
#[derive(Debug, Serialize, Deserialize)]
pub struct SecretChangeKeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamedSecretWithValue {
    pub key: String,
    pub new_key: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedSecretWithValues {
    pub key: String,
    pub old: String,
    pub new: String,
}

// NO values
#[derive(Debug, Serialize, Deserialize)]
pub struct SecretsChangeNoValues {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed: Option<Vec<RenamedSecret>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamedSecret {
    pub key: String,
    pub new_key: String,
}

// NO values end

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum EnvChangelogItemChange {
    Locked {
        locked: bool,
    },
    TypeChange {
        #[serde(rename = "oldType")]
        old_type: EnvType,

        #[serde(rename = "newType")]
        new_type: EnvType,
    },
    Renamed {
        #[serde(rename = "newName")]
        new_name: String,

        #[serde(rename = "oldName")]
        old_name: String,
    },
    Created {
        action: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum EnvChangelogItemSecretsAction {
    Renamed {
        key: String,

        #[serde(rename = "newKey")]
        new_key: String,

        value: String,
    },
    Updated {
        key: String,
        old: String,
        new: String,
    },
    Created {
        key: String,
        new: String,
    },
    Deleted {
        key: String,
        old: String,
    },
}

impl Display for EnvChangelogList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list_string = self
            .data
            .iter()
            .map(|item| format!("{}", item))
            .collect::<Vec<String>>()
            .join("\n");

        writeln!(f, "{}", list_string)?;

        // writeln!(f, "{} {}", "Has more:".green(), self.has_more)?;
        let page = self.page.unwrap_or(1);

        writeln!(f, "{} {}/{}", "Pages:".green(), page, self.pages)?;

        Ok(())
    }
}

impl Display for EnvChangelogListItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", "Id:".green(), self.id)?;

        if let Some(user) = &self.user {
            writeln!(f, "{} {}", "User:".green(), user.name)?;
        }

        let (formatted, relative) = get_human_datetime(&self.created_at);

        writeln!(f, "{} {} ({})", "Date:".green(), formatted, relative)?;
        writeln!(f, "{} {}", "Change:".green(), self.change)?;

        if let EnvChangelogChange::SecretsChange(change) = &self.change {
            match change {
                SecretsChange::NoValues(change) => {
                    // renamed
                    if let Some(renamed) = &change.renamed {
                        if !renamed.is_empty() {
                            writeln!(f, "\n{}", "Renamed".blue())?;

                            let renamed_string = renamed
                                .into_iter()
                                .map(|item| format!("{}", item))
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", renamed_string)?;
                        }
                    }

                    // updated
                    if let Some(updated) = &change.updated {
                        if !updated.is_empty() {
                            writeln!(f, "\n{}", "Updated".yellow())?;

                            let updated_string = updated
                                .into_iter()
                                .map(|item| format!("{}", item))
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", updated_string)?;
                        }
                    }

                    if let Some(new) = &change.new {
                        if !new.is_empty() {
                            writeln!(f, "\n{}", "Created".green())?;

                            let new_string = new
                                .into_iter()
                                .map(|item| format!("{}", item))
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", new_string)?;
                        }
                    }

                    if let Some(deleted) = &change.deleted {
                        if !deleted.is_empty() {
                            writeln!(f, "\n{}", "Deleted".red())?;

                            let deleted_string = deleted
                                .into_iter()
                                .map(|item| format!("{}", item))
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", deleted_string)?;
                        }
                    }

                    write!(f, "\n")?;
                }
                SecretsChange::WithValues(data) => {
                    if let Some(renamed) = &data.renamed {
                        if !renamed.is_empty() {
                            // writeln!(f, "\n{}", "Renamed".blue())?;
                            writeln!(f)?;

                            let renamed_string = renamed
                                .into_iter()
                                .map(|item| {
                                    format!(
                                        "{} {} {} \n value: {}",
                                        item.key.yellow(),
                                        "->".yellow(),
                                        item.new_key.yellow(),
                                        item.value
                                    )
                                })
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", renamed_string)?;
                        }
                    }

                    if let Some(updated) = &data.updated {
                        if !updated.is_empty() {
                            // writeln!(f, "\n{}", "Updated".yellow())?;
                            writeln!(f)?;

                            let updated_string = updated
                                .into_iter()
                                .map(|item| {
                                    format!(
                                        "{} \n {} {}\n {} {}",
                                        item.key,
                                        "- old:".red(),
                                        item.old,
                                        "+ new:".green(),
                                        item.new
                                    )
                                })
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", updated_string)?;
                        }
                    }

                    if let Some(new) = &data.new {
                        if !new.is_empty() {
                            // writeln!(f, "\n{}", "New".green())?;
                            writeln!(f)?;

                            let new_string = new
                                .into_iter()
                                .map(|item| {
                                    format!(
                                        "{} {}{} {}",
                                        "+".green(),
                                        item.key.green(),
                                        ":".green(),
                                        item.value
                                    )
                                })
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", new_string)?;
                        }
                    }

                    if let Some(deleted) = &data.deleted {
                        if !deleted.is_empty() {
                            // writeln!(f, "\n{}", "New".green())?;
                            // writeln!(f)?;

                            let new_string = deleted
                                .into_iter()
                                .map(|item| {
                                    format!(
                                        "{} {}{} {}",
                                        "-".red(),
                                        item.key.red(),
                                        ":".red(),
                                        item.value
                                    )
                                })
                                .collect::<Vec<String>>()
                                .join("\n");

                            writeln!(f, "{}", new_string)?;
                        }
                    }

                    write!(f, "\n")?;
                }
            }
        }

        Ok(())
    }
}

impl Display for RenamedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.key, self.new_key)?;
        Ok(())
    }
}

impl Display for EnvChangelogChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvChangelogChange::Change(change) => write!(f, "{change}"),
            EnvChangelogChange::SecretsChange(_) => write!(f, "Secrets modified"),
        }?;

        Ok(())
    }
}

impl Display for EnvChangelogItemChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            EnvChangelogItemChange::Locked { locked } => {
                if *locked {
                    "Environment locked".to_string()
                } else {
                    "Environment unlocked".to_string()
                }
            }
            EnvChangelogItemChange::TypeChange { old_type, new_type } => {
                format!("Environment type changed from {} to {}", old_type, new_type)
            }
            EnvChangelogItemChange::Renamed { new_name, old_name } => {
                format!("Environment renamed from {} to {}", old_name, new_name)
            }
            EnvChangelogItemChange::Created { action: _ } => {
                format!("Environment created",)
            }
        };

        write!(f, "{msg}")?;
        Ok(())
    }
}

// TODO: table print ???
impl Display for EnvChangelogItemSecretsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvChangelogItemSecretsAction::Renamed {
                key,
                new_key,
                value: _,
            } => {
                // write!(f, "{key} {} {}", "|+/-|".yellow(), new_key,)?;
                // write!(f, "{key} {} {} | {}", "->".yellow(), new_key, "••••••••")?;
                write!(f, "{key} {} {}", "->".yellow(), new_key,)?;
            }
            EnvChangelogItemSecretsAction::Created { key, new: _ } => {
                write!(f, "{key}: {} {}", "|+|".green(), "••••••••")?;
            }
            EnvChangelogItemSecretsAction::Deleted { key, old: _ } => {
                write!(f, "{key}: {} {}", "|+|".red(), "••••••••")?;
            }
            EnvChangelogItemSecretsAction::Updated {
                key,
                old: _,
                new: _,
            } => {
                write!(
                    f,
                    "{key}: {} {} {} {}",
                    "|-|".red(),
                    "••••••••",
                    "|+|".green(),
                    "••••••••"
                )?;
            }
        };

        Ok(())
    }
}
