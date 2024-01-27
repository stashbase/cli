use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
}

impl Config {
    pub fn new() -> Self {
        Self { api_key: None }
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
}
