use core::fmt;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cmd::{pull::PullFormat, push::PushFormat};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfigItem {
    pub project: String,
    pub environment: String,
    pub description: Option<String>,

    pub file: Option<String>,
    pub format: Option<PullFormat>,
    pub secrets: Option<PullSecretsConfig>,

    pub pull: Option<PullActionConfig>,

    // only for push
    pub push: Option<PushActionConfig>,
}

impl EnvConfigItem {
    pub fn get_push_target_file(&self) -> Option<String> {
        match &self.push {
            Some(p) => match &p.file {
                Some(_) => p.file.to_owned(),
                None => self.file.to_owned(),
            },
            None => self.file.to_owned(),
        }
    }

    pub fn get_pull_target_file(&self) -> Option<String> {
        match &self.pull {
            Some(p) => match &p.file {
                Some(_) => p.file.to_owned(),
                None => self.file.to_owned(),
            },
            None => self.file.to_owned(),
        }
    }

    pub fn get_push_format(&self) -> Option<PushFormat> {
        match &self.push {
            Some(p) => match &p.format {
                Some(_) => p.format.to_owned(),
                None => self.format.to_owned(),
            },
            None => self.format.to_owned(),
        }
    }

    pub fn get_pull_format(&self) -> Option<PullFormat> {
        match &self.pull {
            Some(p) => match &p.format {
                Some(_) => p.format.to_owned(),
                None => self.format.to_owned(),
            },
            None => self.format.to_owned(),
        }
    }

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

    pub fn get_pull_secrets(&self) -> PullSecretsConfig {
        let mut exclude: Option<Vec<String>> = None;
        let mut only: Option<Vec<String>> = None;
        let mut set: Option<HashMap<String, String>> = None;
        let mut expand_refs: Option<bool> = None;
        let mut print_secrets: Option<bool> = None;

        match &self.pull {
            Some(push) => match &push.secrets {
                Some(pull_secrets) => {
                    exclude = pull_secrets.exclude.to_owned();
                    only = pull_secrets.only.to_owned();
                    set = pull_secrets.set.to_owned();
                    expand_refs = pull_secrets.expand_refs;
                    print_secrets = pull_secrets.print;
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

        PullSecretsConfig::new(only, exclude, set, expand_refs, print_secrets)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfig {
    pub file: String,
    pub format: Option<PullFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    pub file: Option<String>,
    pub format: Option<PullFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushActionConfig {
    pub file: Option<String>,
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
pub struct PullActionConfig {
    pub file: Option<String>,
    pub format: Option<PullFormat>,
    // Overwrite existing file without prompt
    pub overwrite: Option<bool>,
    pub secrets: Option<PullSecretsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullSecretsConfig {
    pub print: Option<bool>,
    // Select secret keys
    pub only: Option<Vec<String>>,
    // Exclude secret keys
    pub exclude: Option<Vec<String>>,
    pub set: Option<HashMap<String, String>>,

    #[serde(rename = "expand-refs")]
    pub expand_refs: Option<bool>,
}

impl PullSecretsConfig {
    fn new(
        only: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        set: Option<HashMap<String, String>>,
        expand_refs: Option<bool>,
        print: Option<bool>,
    ) -> Self {
        Self {
            only,
            exclude,
            print,
            set,
            expand_refs,
        }
    }
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
