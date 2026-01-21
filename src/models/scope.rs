use clap::ValueEnum;
use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, ValueEnum)]
/// API scope [default: workspace]
pub enum Scope {
    /// Uses --project/--environment or config defaults
    #[default]
    #[clap(alias = "ws")]
    Workspace,
    /// No project/environment flags allowed, requires environment-scoped API authentication
    #[clap(alias = "env")]
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
