use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A trusted, user-configured capability profile for a local coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub project: String,
    pub environment: String,
    pub secrets: HashMap<String, AgentSecretProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSecretProfile {
    pub hosts: Vec<String>,
}
