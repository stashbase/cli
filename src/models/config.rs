use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::cmd::config::{OutputFormat, SecretsOutputFormat};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub ouput_format: Option<OutputFormatConfig>,
    pub state: Option<State>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputFormatConfig {
    pub general: Option<OutputFormat>,
    pub secrets: Option<SecretsOutputFormat>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub project: Option<String>,
    pub environment: Option<String>,
}

impl OutputFormatConfig {
    pub fn new() -> Self {
        Self {
            general: None,
            secrets: None,
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            api_key: None,
            ouput_format: None,
            state: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        if let Some(output_format) = &self.ouput_format {
            self.api_key.is_none() && output_format.is_empty()
        } else {
            self.api_key.is_none() && self.ouput_format.is_none()
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            project: None,
            environment: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.project.is_none() && self.environment.is_none()
    }
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.project.is_none() && self.environment.is_none() {
            writeln!(f, "{}", "No state set")?;
        } else {
            let mut text = "".to_string();

            if let Some(project) = &self.project {
                text.push_str(format!("Project: {}", project).as_str());
            }

            if let Some(environment) = &self.environment {
                if text != "" {
                    text.push_str(format!("; environment: {}", environment).as_str());
                } else {
                    text.push_str(format!("Environment: {}", environment).as_str());
                }
            }

            writeln!(f, "{}", text)?;
        }

        Ok(())
    }
}

impl OutputFormatConfig {
    pub fn is_empty(&self) -> bool {
        self.general.is_none() && self.secrets.is_none()
    }
}

#[derive(Debug)]
pub struct ConfigWithPath {
    pub config_dir: String,
    pub config: Config,
}

#[derive(Debug)]
pub struct UpdateConfig {
    pub api_key: Option<String>,
    pub output_format: Option<OutputFormatConfig>,
    pub state: Option<State>,
}
