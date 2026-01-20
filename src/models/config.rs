use serde::{Deserialize, Serialize};

use crate::cmd::{
    config::{OutputFormat, SecretsOutputFormat},
    shared::Scope,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub scope: Option<Scope>,
    pub api_key: Option<String>,
    pub expand_refs: Option<bool>,
    pub ouput_format: Option<OutputFormatConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            scope: None,
            api_key: None,
            ouput_format: None,
            expand_refs: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        if let Some(output_format) = &self.ouput_format {
            self.api_key.is_none()
                && self.scope.is_none()
                && output_format.is_empty()
                && self.expand_refs.is_none()
        } else {
            self.api_key.is_none()
                && self.scope.is_none()
                && self.ouput_format.is_none()
                && self.expand_refs.is_none()
        }
    }
}

impl OutputFormatConfig {
    pub fn is_empty(&self) -> bool {
        self.general.is_none() && self.secrets.is_none()
    }
}

#[derive(Debug)]
pub struct UpdateConfig {
    pub scope: Option<Scope>,
    pub api_key: Option<String>,
    pub expand_refs: Option<bool>,
    pub output_format: Option<OutputFormatConfig>,
}
