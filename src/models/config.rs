use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    cmd::config::{OutputFormat, SecretsOutputFormat},
    models::agent::{AgentProfile, AgentProfileRestrictions},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub expand_refs: Option<bool>,
    pub ouput_format: Option<OutputFormatConfig>,
    pub agent_profiles: Option<HashMap<String, AgentProfile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFormatConfig {
    pub general: Option<OutputFormat>,
    pub secrets: Option<SecretsOutputFormat>,
}

/// The deliberately limited schema for a repository-local `.stashbase.toml`.
/// It is not allowed to contain global credentials or agent secret sources.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub agent_profiles: Option<HashMap<String, AgentProfileRestrictions>>,
}

impl OutputFormatConfig {
    pub fn new() -> Self {
        Self {
            general: None,
            secrets: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ProjectConfig};

    #[test]
    fn parses_agent_profile_with_secret_host_allowlist() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.coding]
                project = "project"
                environment = "development"

                [agent_profiles.coding.secrets.GH_TOKEN]
                hosts = ["api.github.com"]
            "#,
        )
        .unwrap();

        let profile = &config.agent_profiles.unwrap()["coding"];
        assert_eq!(profile.project.as_deref(), Some("project"));
        assert_eq!(profile.secrets["GH_TOKEN"].hosts, ["api.github.com"]);
    }

    #[test]
    fn parses_agent_profile_with_local_file_source() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.local]
                file = "/tmp/agent.env"
                egress_hosts = ["registry.npmjs.org"]

                [agent_profiles.local.secrets.GH_TOKEN]
                hosts = ["api.github.com"]
            "#,
        )
        .unwrap();

        let profile = &config.agent_profiles.unwrap()["local"];
        assert_eq!(profile.file.as_deref(), Some("/tmp/agent.env"));
        assert!(profile.project.is_none());
        assert_eq!(
            profile.egress_hosts.as_ref().unwrap(),
            &vec!["registry.npmjs.org".to_owned()]
        );
    }

    #[test]
    fn parses_project_agent_restrictions_without_secret_sources() {
        let config: ProjectConfig = toml::from_str(
            r#"
                [agent_profiles.coding]
                egress_hosts = ["registry.npmjs.org"]

                [agent_profiles.coding.secrets.GH_TOKEN]
                hosts = ["api.github.com"]
            "#,
        )
        .unwrap();

        let profile = &config.agent_profiles.unwrap()["coding"];
        assert_eq!(
            profile.secrets.as_ref().unwrap()["GH_TOKEN"].hosts,
            ["api.github.com"]
        );
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            api_key: None,
            ouput_format: None,
            agent_profiles: None,
            expand_refs: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        if let Some(output_format) = &self.ouput_format {
            self.api_key.is_none()
                && output_format.is_empty()
                && self.expand_refs.is_none()
                && self.agent_profiles.is_none()
        } else {
            self.api_key.is_none()
                && self.ouput_format.is_none()
                && self.expand_refs.is_none()
                && self.agent_profiles.is_none()
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
    pub api_key: Option<String>,
    pub expand_refs: Option<bool>,
    pub output_format: Option<OutputFormatConfig>,
}
