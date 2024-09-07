use core::fmt;
use std::{collections::HashMap, env};

use anyhow::{bail, Result};
use log::debug;
use serde::{Deserialize, Serialize};

use crate::{
    cmd::{pull::PullFormat, push::PushFormat},
    models::validation::{InputValidationError, YamlEnvConfigError},
    utils::interaction::select,
};

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
    pub fn select_from_file(
        relative_path: Option<String>,
        config_action_command: &ConfigActionCommand,
    ) -> Result<Option<EnvConfigItem>> {
        return select_from_file(relative_path, config_action_command);
    }

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
        self.get_pull_run_secrets(false)
    }

    pub fn get_run_secrets(&self) -> PullSecretsConfig {
        self.get_pull_run_secrets(true)
    }

    fn get_pull_run_secrets(&self, is_run_action: bool) -> PullSecretsConfig {
        let self_secrets = self.secrets.as_ref();
        let mut exclude: Option<Vec<String>> = self_secrets.and_then(|s| s.exclude.to_owned());
        let mut only: Option<Vec<String>> = self_secrets.and_then(|s| s.only.to_owned());
        let mut set: Option<HashMap<String, String>> = self_secrets.and_then(|s| s.set.to_owned());

        let mut expand_refs: Option<bool> = None;
        let mut print_secrets: Option<bool> = None;

        let action_secrets = match is_run_action {
            true => self.run.as_ref().and_then(|r| r.secrets.to_owned()),
            false => self.pull.as_ref().and_then(|p| p.secrets.to_owned()),
        };

        if let Some(action_secrets) = action_secrets {
            if let Some(ex) = action_secrets.exclude.to_owned() {
                exclude = Some(ex.to_owned());
            }

            if let Some(on) = action_secrets.only.to_owned() {
                only = Some(on.to_owned());
            }

            if let Some(s) = action_secrets.set.to_owned() {
                set = Some(s.to_owned());
            }

            expand_refs = action_secrets.expand_refs;
            print_secrets = action_secrets.print;
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

pub fn select_from_file(
    relative_path: Option<String>,
    config_action_command: &ConfigActionCommand,
) -> Result<Option<EnvConfigItem>> {
    // Load from file
    let file_path = match &relative_path {
        Some(relative_path) => {
            let mut path = std::env::current_dir()?;
            path.push(relative_path);
            path
        }
        None => env::current_dir()?.join("stashbase.yaml"),
    };
    let file_exists = file_path.exists();

    if !file_exists {
        let file_not_found_error = YamlEnvConfigError::FileNotFound {
            custom_path: if relative_path.is_some() { true } else { false },
        };

        let err = InputValidationError::YamlConfigFile(file_not_found_error);
        bail!(err);
    } else {
        let file_content_res = std::fs::read_to_string(file_path);

        if let Err(e) = file_content_res {
            let failed_to_read_err = YamlEnvConfigError::FailedToRead {
                custom_path: if relative_path.is_some() { true } else { false },
                message: e.to_string(),
            };

            let err = InputValidationError::YamlConfigFile(failed_to_read_err);
            bail!(err);
        }

        let file_content = file_content_res.unwrap();
        let deserialized_config_res = serde_yaml::from_str::<Vec<EnvConfigItem>>(&file_content);

        if let Err(e) = deserialized_config_res {
            let failed_to_read_err = YamlEnvConfigError::FailedToRead {
                custom_path: if relative_path.is_some() { true } else { false },
                message: e.to_string(),
            };

            let err = InputValidationError::YamlConfigFile(failed_to_read_err);
            bail!(err);
        }

        let deserialized_config = deserialized_config_res.unwrap();
        let len = deserialized_config.len();

        if len == 0 {
            let err = InputValidationError::YamlConfigFile(YamlEnvConfigError::NoEntries);
            bail!(err);
        } else {
            if len == 1 {
                let item = deserialized_config[0].clone();
                return Ok(Some(item));
            } else {
                let items = deserialized_config
                    .iter()
                    .map(|item| item.get_print_string(config_action_command))
                    .collect();
                // select project
                let selection = select("Select environment config", items);

                debug!("selection: {:?}", selection);

                if let Some(selection) = selection {
                    let item = deserialized_config[selection].clone();
                    debug!("item: {:?}", item);

                    return Ok(Some(item));
                } else {
                    return Ok(None);
                }
            }
        }
    }
}
