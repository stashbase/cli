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

    pub run: Option<RunActionConfig>,
    pub pull: Option<PullActionConfig>,

    // only for push
    pub push: Option<PushActionConfig>,
}

pub enum ConfigActionCommand {
    Pull,
    Push,
    Run,
}

impl EnvConfigItem {
    pub fn get_print_string(&self, config_action_command: &ConfigActionCommand) -> String {
        let mut args_string = String::new();

        let secrets = match config_action_command {
            ConfigActionCommand::Pull => {
                let secrets = self.get_pull_secrets();
                (secrets.only, secrets.exclude, secrets.set)
            }
            ConfigActionCommand::Push => {
                let secrets = self.get_push_secrets();
                (secrets.only, secrets.exclude, secrets.set)
            }
            ConfigActionCommand::Run => match &self.secrets {
                Some(s) => (s.only.to_owned(), s.exclude.to_owned(), s.set.to_owned()),
                None => (None, None, None),
            },
        };

        let only = &secrets.0;
        let exclude = &secrets.1;
        let set = &secrets.2;

        if let Some(only) = only {
            args_string.push_str(&format!("only ({})", only.len()));
        }

        if let Some(exclude) = exclude {
            if args_string != "" {
                args_string.push_str(", ");
            }
            args_string.push_str(&format!("exclude ({})", exclude.len()));
        }

        if let Some(set) = set {
            if args_string != "" {
                args_string.push_str(", ");
            }
            args_string.push_str(&format!("set ({})", set.len()));
        }

        let str = match &self.description {
            Some(description) => {
                if args_string.len() > 0 {
                    format!(
                        "{} -> {} | {}\n   🗎 {}",
                        self.project, self.environment, args_string, description
                    )
                } else {
                    format!(
                        "{} -> {}\n   🗎 {}",
                        self.project, self.environment, description
                    )
                }
            }
            None => {
                if args_string.len() > 0 {
                    format!("{} -> {} | {}", self.project, self.environment, args_string)
                } else {
                    format!("{} -> {}", self.project, self.environment)
                }
            }
        };

        return str;
    }

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
        let self_secrets = self.secrets.as_ref();
        let mut exclude: Option<Vec<String>> = self_secrets.and_then(|s| s.exclude.to_owned());
        let mut only: Option<Vec<String>> = self_secrets.and_then(|s| s.only.to_owned());
        let mut set: Option<HashMap<String, String>> = self_secrets.and_then(|s| s.set.to_owned());

        if let Some(p) = &self.push {
            if let Some(p_secrets) = &p.secrets {
                if let Some(ex) = p_secrets.exclude.to_owned() {
                    exclude = Some(ex.to_owned());
                }

                if let Some(on) = p_secrets.only.to_owned() {
                    only = Some(on.to_owned());
                }

                if let Some(s) = p_secrets.set.to_owned() {
                    set = Some(s.to_owned());
                }
            }
        }

        PushSecretsConfig::new(only, exclude, set)
    }

    pub fn get_pull_secrets(&self) -> PullSecretsConfig {
        let self_secrets = self.secrets.as_ref();
        let mut exclude: Option<Vec<String>> = self_secrets.and_then(|s| s.exclude.to_owned());
        let mut only: Option<Vec<String>> = self_secrets.and_then(|s| s.only.to_owned());
        let mut set: Option<HashMap<String, String>> = self_secrets.and_then(|s| s.set.to_owned());

        let mut expand_refs: Option<bool> = None;
        let mut print_secrets: Option<bool> = None;

        match &self.pull {
            Some(push) => match &push.secrets {
                Some(pull_secrets) => {
                    if let Some(ex) = pull_secrets.exclude.to_owned() {
                        exclude = Some(ex.to_owned());
                    }
                    if let Some(on) = pull_secrets.only.to_owned() {
                        only = Some(on.to_owned());
                    }

                    if let Some(s) = pull_secrets.set.to_owned() {
                        set = Some(s.to_owned());
                    }

                    expand_refs = pull_secrets.expand_refs;
                    print_secrets = pull_secrets.print;
                }
                None => match &self.secrets {
                    Some(s) => {
                        if let Some(ex) = s.exclude.to_owned() {
                            exclude = Some(ex.to_owned());
                        }
                        if let Some(on) = s.only.to_owned() {
                            only = Some(on.to_owned());
                        }
                    }
                    _ => {}
                },
            },
            None => match &self.secrets {
                Some(s) => {
                    if let Some(ex) = s.exclude.to_owned() {
                        exclude = Some(ex.to_owned());
                    }

                    if let Some(on) = s.only.to_owned() {
                        only = Some(on.to_owned());
                    }
                }
                None => {}
            },
        }

        PullSecretsConfig::new(only, exclude, set, expand_refs, print_secrets)
    }
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
    fn new(
        only: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        set: Option<HashMap<String, String>>,
    ) -> Self {
        Self { only, exclude, set }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSecretsConfig {
    pub only: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub set: Option<HashMap<String, String>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunActionConfig {
    // pub file: Option<String>,
    pub secrets: Option<PullSecretsConfig>,
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
