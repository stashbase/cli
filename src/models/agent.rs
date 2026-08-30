use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A trusted, user-configured capability profile for a local coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub file: Option<String>,
    pub egress_hosts: Option<Vec<String>>,
    /// Destinations denied after both secret and ordinary egress rules are evaluated.
    pub deny_hosts: Option<Vec<String>>,
    /// Commands that should be denied when launched through the agent's PATH.
    #[serde(default)]
    pub commands: AgentCommandsProfile,
    /// Filesystem paths denied to the agent process tree.
    #[serde(default)]
    pub filesystem: AgentFilesystemProfile,
    #[serde(default)]
    pub secrets: AgentSecretsProfile,
    /// Personal credentials are private to the authenticated account and are
    /// resolved only by Remote Agent sessions. The CLI never reads or persists
    /// their values.
    #[serde(default, alias = "credentials")]
    pub personal_credentials: HashMap<String, AgentBindingProfile>,
    /// Optional local-only regression cases for this profile's HTTP policy.
    #[serde(default)]
    pub policy_tests: Vec<AgentPolicyTestCase>,
}

/// Local command restrictions for an agent session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCommandsProfile {
    /// Executable names to shadow with denying wrappers in the child PATH.
    #[serde(default)]
    pub denied: Vec<String>,
    /// Executable/argv patterns denied by the command wrapper layer.
    #[serde(default)]
    pub denied_with_args: Vec<AgentCommandDenyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCommandDenyRule {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    #[serde(rename = "match")]
    pub match_mode: AgentArgumentMatch,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentArgumentMatch {
    #[default]
    Exact,
    Contains,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentFilesystemProfile {
    /// Paths the agent must not read.
    #[serde(default)]
    pub deny_read: Vec<String>,
    /// Paths the agent must not modify.
    #[serde(default)]
    pub deny_write: Vec<String>,
}

/// Project/environment-backed secret bindings. Personal credentials deliberately
/// live outside this table because they are owned by the authenticated account.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentSecretsProfile {
    pub project: Option<String>,
    pub environment: Option<String>,
    #[serde(flatten)]
    pub bindings: HashMap<String, AgentBindingProfile>,
}

impl From<HashMap<String, AgentBindingProfile>> for AgentSecretsProfile {
    fn from(bindings: HashMap<String, AgentBindingProfile>) -> Self {
        Self {
            project: None,
            environment: None,
            bindings,
        }
    }
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
pub struct AgentBindingProfile {
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
    /// Template for the header value. `{value}` is replaced only inside the proxy.
    /// Defaults to `Bearer {value}` for Authorization and `{value}` for other headers.
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
