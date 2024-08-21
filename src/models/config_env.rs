use core::fmt;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cmd::pull::PullFormat;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfigItem {
    pub project: String,
    pub environment: String,
    pub description: Option<String>,

    pub secrets: Option<EnvConfigItemSecrets>,
    pub pull: Option<ActionConfig>,

    // both for push/pull
    pub target: Option<ActionConfig>,

    // only for push
    pub push: Option<PushActionConfig>,
}

impl EnvConfigItem {
    pub fn get_push_secrets(&self) -> PushSecretsConfig {
        let mut exclude: Option<Vec<String>> = None;
        let mut only: Option<Vec<String>> = None;

        match &self.push {
            Some(push) => match &push.secrets {
                Some(push_secrets) => {
                    exclude = push_secrets.exclude.to_owned();
                    only = push_secrets.only.to_owned();
                }
                None => match &self.secrets {
                    Some(s) => {
                        exclude = s.exclude.to_owned();
                        only = s.only.to_owned();
                    }
                    _ => {}
                },
            },
            None => match &self.secrets {
                Some(s) => {
                    exclude = s.exclude.to_owned();
                    only = s.only.to_owned();
                }
                None => {}
            },
        }

        PushSecretsConfig::new(only, exclude)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfig {
    #[serde(rename = "path")]
    pub file: String,
    pub format: Option<PullFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushActionConfig {
    #[serde(rename = "path")]
    pub file: String,
    pub format: Option<PullFormat>,
    pub secrets: Option<PushSecretsConfig>,
}

impl PushSecretsConfig {
    fn new(only: Option<Vec<String>>, exclude: Option<Vec<String>>) -> Self {
        Self { only, exclude }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSecretsConfig {
    pub only: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfigItemSecrets {
    pub print: Option<bool>,
    // Select secret keys
    pub only: Option<Vec<String>>,
    // Exclude secret keys
    pub exclude: Option<Vec<String>>,
    pub set: Option<HashMap<String, String>>,

    #[serde(rename = "expand-refs")]
    pub expand_refs: Option<bool>,
}

impl fmt::Display for EnvConfigItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let args_str = match &self.secrets {
            Some(s) => match (&s.only, &s.exclude) {
                (Some(only), Some(exclude)) => {
                    let only_len = only.len();
                    let exclude_len = exclude.len();

                    Some(format!("only - ({}), exclude ({})", only_len, exclude_len))
                }
                (None, None) => None,
                (None, Some(exclude)) => {
                    let exclude_len = exclude.len();

                    Some(format!("exclude ({})", exclude_len))
                }
                (Some(only), None) => {
                    let only_len = only.len();
                    Some(format!("only ({})", only_len))
                }
            },
            None => None,
        };

        let str = match &self.description {
            Some(description) => {
                if let Some(args) = args_str {
                    format!(
                        "{} -> {} | {}\n   🗎 {}",
                        self.project, self.environment, args, description
                    )
                } else {
                    format!(
                        "{} -> {}\n   🗎 {}",
                        self.project, self.environment, description
                    )
                }
            }
            None => {
                if let Some(args) = args_str {
                    format!("{} -> {} | {}", self.project, self.environment, args)
                } else {
                    format!("{} -> {}", self.project, self.environment)
                }
            }
        };

        write!(f, "{}", str)?;

        Ok(())
    }
}
