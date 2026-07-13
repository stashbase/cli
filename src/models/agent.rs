use std::collections::HashMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// A trusted, user-configured capability profile for a local coding agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub project: Option<String>,
    pub environment: Option<String>,
    pub file: Option<String>,
    pub egress_hosts: Option<Vec<String>>,
    pub secrets: HashMap<String, AgentSecretProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSecretProfile {
    pub hosts: Vec<String>,
}

/// Repository-owned restrictions for a user-owned agent profile. This type
/// intentionally has no credential-source fields: a checked-out repository
/// may narrow access but cannot introduce a new secret source or broaden one.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileRestrictions {
    pub egress_hosts: Option<Vec<String>>,
    pub secrets: Option<HashMap<String, AgentSecretProfile>>,
}

impl AgentProfile {
    pub fn restricted_by(&self, restrictions: &AgentProfileRestrictions) -> Result<Self> {
        let mut profile = self.clone();

        if let Some(egress_hosts) = &restrictions.egress_hosts {
            let allowed = self.egress_hosts.as_deref().unwrap_or_default();
            if !hosts_are_subset(egress_hosts, allowed) {
                bail!("Project agent config may only narrow egress_hosts from the user profile.");
            }
            profile.egress_hosts = Some(egress_hosts.clone());
        }

        if let Some(secrets) = &restrictions.secrets {
            for (name, restricted_secret) in secrets {
                let Some(allowed_secret) = self.secrets.get(name) else {
                    bail!("Project agent config may not add secret '{name}' to a user profile.");
                };
                if !hosts_are_subset(&restricted_secret.hosts, &allowed_secret.hosts) {
                    bail!("Project agent config may only narrow hosts for secret '{name}'.");
                }
            }
            profile.secrets = secrets.clone();
        }

        Ok(profile)
    }
}

fn hosts_are_subset(restricted: &[String], allowed: &[String]) -> bool {
    restricted.iter().all(|host| {
        allowed.iter().any(|allowed_host| {
            host.trim()
                .trim_end_matches('.')
                .eq_ignore_ascii_case(allowed_host.trim().trim_end_matches('.'))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{AgentProfile, AgentProfileRestrictions, AgentSecretProfile};
    use std::collections::HashMap;

    fn user_profile() -> AgentProfile {
        AgentProfile {
            project: Some("project".to_owned()),
            environment: Some("development".to_owned()),
            file: None,
            egress_hosts: Some(vec!["registry.npmjs.org".to_owned()]),
            secrets: HashMap::from([(
                "GH_TOKEN".to_owned(),
                AgentSecretProfile {
                    hosts: vec!["api.github.com".to_owned()],
                },
            )]),
        }
    }

    #[test]
    fn project_restrictions_can_remove_unused_secrets() {
        let profile = user_profile();
        let restrictions = AgentProfileRestrictions {
            egress_hosts: Some(vec!["registry.npmjs.org".to_owned()]),
            secrets: Some(HashMap::new()),
        };

        let restricted = profile.restricted_by(&restrictions).unwrap();
        assert!(restricted.secrets.is_empty());
    }

    #[test]
    fn project_restrictions_cannot_add_a_secret_or_host() {
        let profile = user_profile();
        let restrictions = AgentProfileRestrictions {
            egress_hosts: Some(vec!["example.com".to_owned()]),
            secrets: Some(HashMap::from([(
                "OTHER_TOKEN".to_owned(),
                AgentSecretProfile {
                    hosts: vec!["example.com".to_owned()],
                },
            )])),
        };

        assert!(profile.restricted_by(&restrictions).is_err());
    }
}
