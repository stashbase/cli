use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    cmd::config::{OutputFormat, SecretsOutputFormat},
    models::agent::AgentProfile,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    /// The profile used when neither `--profile` nor `STASHBASE_PROFILE` is set.
    pub default_profile: Option<String>,
    /// Non-sensitive profile metadata. Profile API keys live in the OS secure store.
    pub profiles: Option<BTreeMap<String, ProfileConfig>>,
    pub expand_refs: Option<bool>,
    pub ouput_format: Option<OutputFormatConfig>,
    pub agent_profiles: Option<HashMap<String, AgentProfile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileConfig {
    /// An optional, human-readable workspace name or slug. Authentication is
    /// determined by the profile's API key, not this value.
    pub workspace: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::models::agent::{AgentArgumentMatch, AgentProfile};

    #[test]
    fn parses_agent_profile_with_argument_aware_command_denials() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.coding.commands]
                denied = ["ssh"]

                [[agent_profiles.coding.commands.denied_with_args]]
                program = "git"
                args = ["push", "--force"]
                match = "contains"

                [[agent_profiles.coding.commands.denied_with_args]]
                program = "npm"
                args = ["publish"]
                match = "exact"
            "#,
        )
        .unwrap();

        let profile = &config.agent_profiles.unwrap()["coding"];
        assert_eq!(profile.commands.denied, ["ssh"]);
        assert_eq!(profile.commands.denied_with_args.len(), 2);
        assert_eq!(
            profile.commands.denied_with_args[0].match_mode,
            AgentArgumentMatch::Contains
        );
        assert_eq!(
            profile.commands.denied_with_args[1].match_mode,
            AgentArgumentMatch::Exact
        );
    }

    #[test]
    fn rejects_invalid_argument_aware_command_denial_match() {
        let error = toml::from_str::<AgentProfile>(
            r#"
                [commands]
                [[commands.denied_with_args]]
                program = "git"
                args = ["push"]
                match = "prefix"
            "#,
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("expected `exact` or `contains`"),
            "{error}"
        );
    }

    #[test]
    fn rejects_unknown_argument_aware_command_denial_fields() {
        let error = toml::from_str::<AgentProfile>(
            r#"
                [[commands.denied_with_args]]
                program = "git"
                args = ["push"]
                unexpected = true
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn parses_agent_profile_with_secret_host_allowlist() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.coding.secrets]
                project = "project"
                environment = "development"

                [agent_profiles.coding.secrets.GH_TOKEN]
                hosts = ["api.github.com"]
                from = "GITHUB_TOKEN"
                env = "GH_TOKEN"
                placeholder = "example-placeholder"
                header = "x-api-key"
                value_template = "Token {value}"
            "#,
        )
        .unwrap();

        let profile = &config.agent_profiles.unwrap()["coding"];
        assert_eq!(profile.secrets.project.as_deref(), Some("project"));
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"].hosts,
            ["api.github.com"]
        );
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"].from.as_deref(),
            Some("GITHUB_TOKEN")
        );
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"].env.as_deref(),
            Some("GH_TOKEN")
        );
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"].placeholder.as_deref(),
            Some("example-placeholder")
        );
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"].header.as_deref(),
            Some("x-api-key")
        );
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"]
                .value_template
                .as_deref(),
            Some("Token {value}")
        );
    }

    #[test]
    fn rejects_top_level_project_and_environment_in_agent_profiles() {
        let error = toml::from_str::<AgentProfile>(
            r#"
                project = "project"
                environment = "development"
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn parses_profile_metadata_without_api_keys() {
        let config: Config = toml::from_str(
            r#"
                default_profile = "acme"

                [profiles.acme]
                workspace = "acme-production"
            "#,
        )
        .unwrap();

        assert_eq!(config.default_profile.as_deref(), Some("acme"));
        assert_eq!(
            config.profiles.unwrap()["acme"].workspace.as_deref(),
            Some("acme-production")
        );
        assert!(config.api_key.is_none());
    }

    #[test]
    fn parses_agent_profile_with_http_action_rules() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.coding.secrets]
                project = "project"
                environment = "development"

                [agent_profiles.coding.secrets.GH_TOKEN]
                [[agent_profiles.coding.secrets.GH_TOKEN.rules]]
                effect = "allow"
                hosts = ["api.github.com"]
                methods = ["get"]
                paths = ["/repos/*"]
            "#,
        )
        .unwrap();

        let rule = &config.agent_profiles.unwrap()["coding"].secrets.bindings["GH_TOKEN"].rules[0];
        assert_eq!(rule.methods, ["get"]);
        assert_eq!(rule.paths, ["/repos/*"]);
    }

    #[test]
    fn parses_agent_profile_with_personal_credentials() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.coding.secrets]
                project = "project"
                environment = "development"

                [agent_profiles.coding.personal_credentials.LINEAR_API_KEY]
                env = "LINEAR_API_KEY"
                [[agent_profiles.coding.personal_credentials.LINEAR_API_KEY.rules]]
                effect = "allow"
                hosts = ["mcp.linear.app"]
                methods = ["GET", "POST"]
                paths = ["/mcp"]
            "#,
        )
        .unwrap();

        let profile = &config.agent_profiles.unwrap()["coding"];
        assert!(profile.secrets.bindings.is_empty());
        assert_eq!(
            profile.personal_credentials["LINEAR_API_KEY"]
                .env
                .as_deref(),
            Some("LINEAR_API_KEY")
        );
        assert_eq!(
            profile.personal_credentials["LINEAR_API_KEY"].rules[0].methods,
            ["GET", "POST"]
        );
    }

    #[test]
    fn parses_embedded_agent_policy_tests() {
        let config: Config = toml::from_str(
            r#"
                [agent_profiles.coding]

                [[agent_profiles.coding.policy_tests]]
                secret = "GH_TOKEN"
                method = "GET"
                host = "api.github.com"
                path = "/user"
                expect = "allow"
            "#,
        )
        .unwrap();

        let test = &config.agent_profiles.unwrap()["coding"].policy_tests[0];
        assert_eq!(test.secret, "GH_TOKEN");
        assert_eq!(test.expect.to_string(), "allow");
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
        assert!(profile.secrets.project.is_none());
        assert_eq!(
            profile.egress_hosts.as_ref().unwrap(),
            &vec!["registry.npmjs.org".to_owned()]
        );
    }

    #[test]
    fn parses_egress_only_agent_profile_without_a_secret_source() {
        let profile: AgentProfile = toml::from_str(
            r#"
                egress_hosts = ["chatgpt.com"]
                deny_hosts = ["api.stashbase.dev"]
            "#,
        )
        .unwrap();

        assert!(profile.file.is_none());
        assert!(profile.secrets.bindings.is_empty());
        assert_eq!(
            profile.deny_hosts.as_ref(),
            Some(&vec!["api.stashbase.dev".to_owned()])
        );
    }

    #[test]
    fn parses_complete_directory_agent_profile() {
        let profile: AgentProfile = toml::from_str(
            r#"
                file = ".env.agent"
                egress_hosts = ["registry.npmjs.org"]

                [secrets.GH_TOKEN]
                hosts = ["api.github.com"]
            "#,
        )
        .unwrap();

        assert_eq!(profile.file.as_deref(), Some(".env.agent"));
        assert_eq!(
            profile.secrets.bindings["GH_TOKEN"].hosts,
            ["api.github.com"]
        );
    }
}

impl Config {
    pub fn new() -> Self {
        Self {
            api_key: None,
            default_profile: None,
            profiles: None,
            ouput_format: None,
            agent_profiles: None,
            expand_refs: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        if let Some(output_format) = &self.ouput_format {
            self.api_key.is_none()
                && output_format.is_empty()
                && self.default_profile.is_none()
                && self.profiles.is_none()
                && self.expand_refs.is_none()
                && self.agent_profiles.is_none()
        } else {
            self.api_key.is_none()
                && self.ouput_format.is_none()
                && self.default_profile.is_none()
                && self.profiles.is_none()
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
