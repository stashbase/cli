use serde::{Deserialize, Serialize};

use crate::cmd::configs::{OutputFormat, OutputFormatSecrets};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub ouput_format: Option<OutputFormatConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputFormatConfig {
    pub general: Option<OutputFormat>,
    pub secrets: Option<OutputFormatSecrets>,
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
        }
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
}
