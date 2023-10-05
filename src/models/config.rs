use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub token: Option<String>,
}

impl Config {
    pub fn new() -> Self {
        Self { token: None }
    }
}

#[derive(Debug)]
pub struct ConfigWithPath {
    pub config_dir: String,
    pub config: Config,
}

#[derive(Debug)]
pub struct UpdateConfig {
    pub token: Option<String>,
}
