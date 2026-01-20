use clap::ValueEnum;
use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, ValueEnum)]
pub enum Scope {
    #[default]
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "environment")]
    Environment,
}

impl Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace => write!(f, "workspace"),
            Self::Environment => write!(f, "environment"),
        }
    }
}
