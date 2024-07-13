use serde::{Deserialize, Serialize};

use crate::cmd::config::{OutputFormat, SecretsOutputFormat};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub replace_refs: Option<bool>,
    pub ouput_format: Option<OutputFormatConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputFormatConfig {
    pub general: Option<OutputFormat>,
    pub secrets: Option<SecretsOutputFormat>,
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
            replace_refs: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        if let Some(output_format) = &self.ouput_format {
            self.api_key.is_none() && output_format.is_empty() && self.replace_refs.is_none()
        } else {
            self.api_key.is_none() && self.ouput_format.is_none() && self.replace_refs.is_none()
        }
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
    pub replace_refs: Option<bool>,
    pub output_format: Option<OutputFormatConfig>,
}
