use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A trusted, user-configured capability profile for a local coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub project: Option<String>,
    pub environment: Option<String>,
    pub file: Option<String>,
    pub egress_hosts: Option<Vec<String>>,
    /// Destinations denied after both secret and ordinary egress rules are evaluated.
    pub deny_hosts: Option<Vec<String>>,
    #[serde(default)]
    pub secrets: HashMap<String, AgentSecretProfile>,
    /// Optional local-only regression cases for this profile's HTTP policy.
    #[serde(default)]
    pub policy_tests: Vec<AgentPolicyTestCase>,
}

/// A declarative expected HTTP credential decision. These are evaluated only by
/// `stashbase agent policy test`; they are never sent to a proxy or server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentPolicyTestCase {
    pub name: Option<String>,
    pub secret: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub expect: AgentPolicyTestExpectation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPolicyTestExpectation {
    Allow,
    Deny,
}

impl std::fmt::Display for AgentPolicyTestExpectation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSecretProfile {
    /// Legacy credential host allowlist, used when `rules` is empty.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Credential-specific HTTP action rules. A matching deny takes precedence.
    #[serde(default)]
    pub rules: Vec<AgentHttpRule>,
    /// Source secret name to request. Defaults to this profile entry's name.
    pub from: Option<String>,
    /// Environment variable exposed to the child. Defaults to this profile entry's name.
    pub env: Option<String>,
    /// Opaque value exposed to clients that validate credential syntax locally.
    /// Defaults to `${STASHBASE_<binding name>}` for remote proxy sessions.
    pub placeholder: Option<String>,
    /// Request header that carries this credential. Defaults to Authorization.
    pub header: Option<String>,
    /// Template for the header value. `{secret}` is replaced only inside the proxy.
    /// Defaults to `Bearer {secret}` for Authorization and `{secret}` for other headers.
    pub value_template: Option<String>,
}

/// An HTTP action rule applied before a secret is injected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHttpRule {
    pub effect: AgentHttpRuleEffect,
    pub hosts: Vec<String>,
    pub methods: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentHttpRuleEffect {
    Allow,
    Deny,
}
