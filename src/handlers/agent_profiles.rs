//! Read-only discovery and inspection of agent profiles.

use std::collections::{BTreeMap, HashSet};

use anyhow::{bail, Result};
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::{
    cmd::agent::{
        AgentProfileSource, AgentProfilesCommand, AgentProfilesListCommand,
        AgentProfilesShowCommand, AgentProfilesSubcommand,
    },
    config::config,
    handlers::agent_policy::{normalize_secret_http_policy, SecretHttpPolicy},
    models::{agent::AgentProfile, config::Config},
    utils::output::{get_formatted_json_string, is_color_enabled},
};

pub fn handle_agent_profiles_command(
    command: AgentProfilesCommand,
    global_config: &Config,
    silent: bool,
    json: bool,
) -> Result<()> {
    match command.subcommand {
        AgentProfilesSubcommand::List(command) => handle_list(command, global_config, silent, json),
        AgentProfilesSubcommand::Show(command) => handle_show(command, global_config, json),
    }
}

fn handle_list(
    command: AgentProfilesListCommand,
    global_config: &Config,
    silent: bool,
    json: bool,
) -> Result<()> {
    let profiles = profiles_for_source(command.profile_source, global_config)?;
    let report = profiles
        .into_iter()
        .map(|(name, (profile, source))| ProfileSummary {
            name,
            source,
            binding_count: binding_count(&profile),
            egress_hosts_configured: profile.egress_hosts.is_some(),
        })
        .collect::<Vec<_>>();
    if !silent {
        println!();
    }
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        if report.is_empty() {
            println!("No agent profiles found.");
            return Ok(());
        }
        println!("Available agent profiles:");
        for profile in report {
            println!(
                "- {} ({}, {} binding{}, {} egress_hosts)",
                profile.name,
                profile.source,
                profile.binding_count,
                if profile.binding_count == 1 { "" } else { "s" },
                if profile.egress_hosts_configured {
                    "configured"
                } else {
                    "not configured"
                },
            );
        }
    }
    Ok(())
}

fn handle_show(
    command: AgentProfilesShowCommand,
    global_config: &Config,
    json: bool,
) -> Result<()> {
    let profiles = if let Some(path) = command.policy_file.as_deref() {
        let profile = config::get_explicit_agent_profile(path)?;
        BTreeMap::from([(command.profile.clone(), (profile.profile, profile.source))])
    } else {
        profiles_for_source(command.profile_source, global_config)?
    };
    let Some((profile, source)) = profiles.get(&command.profile) else {
        bail!(
            "Agent profile '{}' was not found in the {} config.",
            command.profile,
            source_label(command.profile_source)
        );
    };
    let profile = command
        .effective
        .then(|| effective_profile(profile))
        .unwrap_or_else(|| profile.clone());
    let report = ProfileDetails {
        name: command.profile,
        source: source.clone(),
        profile,
    };
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("Agent profile: {}", report.name);
        println!("Profile source: {}", report.source);
        println!();
        print!("{}", format_profile_toml(&report.profile)?);
    }
    Ok(())
}

/// Produces the safe policy shape the local proxy uses after it resolves
/// optional binding fields and canonicalizes host, method, and path matching.
fn effective_profile(profile: &AgentProfile) -> AgentProfile {
    let mut effective = profile.clone();
    effective.egress_hosts = effective.egress_hosts.take().map(normalize_values);
    effective.deny_hosts = effective.deny_hosts.take().map(normalize_values);
    for (name, secret) in &mut effective.secrets.bindings {
        secret.from.get_or_insert_with(|| name.clone());
        secret.env.get_or_insert_with(|| name.clone());
        secret
            .placeholder
            .get_or_insert_with(|| format!("**STASHBASE_{name}**"));
        let header = secret
            .header
            .get_or_insert_with(|| "Authorization".to_owned());
        secret.value_template.get_or_insert_with(|| {
            if header.eq_ignore_ascii_case("authorization") {
                "Bearer {value}".to_owned()
            } else {
                "{value}".to_owned()
            }
        });
        if secret.rules.is_empty() {
            secret.hosts = normalize_values(std::mem::take(&mut secret.hosts));
        } else if let SecretHttpPolicy::Rules(rules) =
            normalize_secret_http_policy(SecretHttpPolicy::Rules(std::mem::take(&mut secret.rules)))
        {
            secret.rules = rules;
            secret.hosts.clear();
        }
    }
    for (name, credential) in &mut effective.personal_credentials {
        credential.from.get_or_insert_with(|| name.clone());
        credential.env.get_or_insert_with(|| name.clone());
        credential
            .placeholder
            .get_or_insert_with(|| format!("**STASHBASE_{name}**"));
        let header = credential
            .header
            .get_or_insert_with(|| "Authorization".to_owned());
        credential.value_template.get_or_insert_with(|| {
            if header.eq_ignore_ascii_case("authorization") {
                "Bearer {value}".to_owned()
            } else {
                "{value}".to_owned()
            }
        });
        if credential.rules.is_empty() {
            credential.hosts = normalize_values(std::mem::take(&mut credential.hosts));
        } else if let SecretHttpPolicy::Rules(rules) = normalize_secret_http_policy(
            SecretHttpPolicy::Rules(std::mem::take(&mut credential.rules)),
        ) {
            credential.rules = rules;
            credential.hosts.clear();
        }
    }
    effective
}

fn normalize_values(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().trim_end_matches('.').to_ascii_lowercase())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn format_profile_toml(profile: &AgentProfile) -> Result<String> {
    let toml = toml::to_string_pretty(profile)?;
    Ok(colorize_toml(&toml, is_color_enabled(true)))
}

/// Small presentation-only highlighter for canonical TOML emitted by the
/// serializer. It is deliberately not used for JSON or file output.
fn colorize_toml(toml: &str, color: bool) -> String {
    if !color {
        return toml.to_owned();
    }
    toml.lines()
        .map(|line| {
            if line.starts_with('[') {
                format!("{}", line.cyan().bold())
            } else if let Some((key, value)) = line.split_once(" = ") {
                format!("{}{}{}", key.blue(), " = ".bright_black(), value)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn profiles_for_source(
    source: AgentProfileSource,
    global_config: &Config,
) -> Result<BTreeMap<String, (AgentProfile, String)>> {
    let global = || {
        global_config
            .agent_profiles
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, profile)| (name, (profile, "user-level config".to_owned())))
            .collect::<BTreeMap<_, _>>()
    };
    let directory = || {
        config::get_directory_agent_profiles().map(|profiles| {
            profiles
                .into_iter()
                .map(|(name, profile)| (name, (profile.profile, profile.source)))
                .collect::<BTreeMap<_, _>>()
        })
    };
    match source {
        AgentProfileSource::Global => Ok(global()),
        AgentProfileSource::Directory => directory(),
        AgentProfileSource::Auto => {
            let mut profiles = global();
            profiles.extend(directory()?);
            Ok(profiles)
        }
    }
}

#[derive(Serialize)]
struct ProfileSummary {
    name: String,
    source: String,
    binding_count: usize,
    egress_hosts_configured: bool,
}

fn binding_count(profile: &AgentProfile) -> usize {
    profile.secrets.bindings.len() + profile.personal_credentials.len()
}

#[derive(Serialize)]
struct ProfileDetails {
    name: String,
    source: String,
    profile: AgentProfile,
}

fn source_label(source: AgentProfileSource) -> &'static str {
    match source {
        AgentProfileSource::Global => "global",
        AgentProfileSource::Directory => "directory",
        AgentProfileSource::Auto => "global or directory",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::models::agent::{AgentBindingProfile, AgentHttpRule, AgentHttpRuleEffect};

    use super::{binding_count, colorize_toml, effective_profile, AgentProfile};

    #[test]
    fn toml_highlighting_preserves_plain_output_when_color_is_disabled() {
        let toml = "[secrets]\nproject = \"local\"\n[secrets.GITHUB_TOKEN]\n";
        assert_eq!(colorize_toml(toml, false), toml);
    }

    #[test]
    fn toml_highlighting_colors_headers_and_keys() {
        let output = colorize_toml(
            "[secrets]\nproject = \"local\"\n[secrets.GITHUB_TOKEN]\n",
            true,
        );
        assert!(output.contains("\u{1b}["));
    }

    #[test]
    fn effective_profile_resolves_defaults_and_normalizes_rules() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: Some(vec!["API.GITHUB.COM.".to_owned()]),
            deny_hosts: None,
            filesystem: Default::default(),
            secrets: HashMap::from([(
                "GITHUB_TOKEN".to_owned(),
                AgentBindingProfile {
                    hosts: Vec::new(),
                    rules: vec![AgentHttpRule {
                        effect: AgentHttpRuleEffect::Allow,
                        hosts: vec!["API.GITHUB.COM.".to_owned()],
                        methods: vec!["get".to_owned()],
                        paths: vec!["/repos/../user".to_owned()],
                    }],
                    from: None,
                    env: None,
                    placeholder: None,
                    header: None,
                    value_template: None,
                },
            )])
            .into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
        };

        let effective = effective_profile(&profile);
        let secret = &effective.secrets.bindings["GITHUB_TOKEN"];
        assert_eq!(secret.from.as_deref(), Some("GITHUB_TOKEN"));
        assert_eq!(secret.env.as_deref(), Some("GITHUB_TOKEN"));
        assert_eq!(secret.header.as_deref(), Some("Authorization"));
        assert_eq!(secret.value_template.as_deref(), Some("Bearer {value}"));
        assert_eq!(secret.rules[0].hosts, ["api.github.com"]);
        assert_eq!(secret.rules[0].methods, ["GET"]);
        assert_eq!(secret.rules[0].paths, ["/user"]);
    }

    #[test]
    fn effective_profile_uses_plain_value_default_for_non_authorization_headers() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            deny_hosts: None,
            filesystem: Default::default(),
            secrets: HashMap::from([(
                "API_KEY".to_owned(),
                AgentBindingProfile {
                    hosts: vec!["API.EXAMPLE.COM.".to_owned()],
                    rules: Vec::new(),
                    from: None,
                    env: None,
                    placeholder: None,
                    header: Some("x-api-key".to_owned()),
                    value_template: None,
                },
            )])
            .into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
        };

        let secret = &effective_profile(&profile).secrets.bindings["API_KEY"];
        assert_eq!(secret.hosts, ["api.example.com"]);
        assert_eq!(secret.value_template.as_deref(), Some("{value}"));
    }

    #[test]
    fn binding_count_includes_secrets_and_personal_credentials() {
        let binding = AgentBindingProfile {
            hosts: Vec::new(),
            rules: Vec::new(),
            from: None,
            env: None,
            placeholder: None,
            header: None,
            value_template: None,
        };
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            deny_hosts: None,
            filesystem: Default::default(),
            secrets: HashMap::from([("GITHUB_TOKEN".to_owned(), binding.clone())]).into(),
            personal_credentials: HashMap::from([("LINEAR_API_KEY".to_owned(), binding)]),
            policy_tests: Vec::new(),
        };

        assert_eq!(binding_count(&profile), 2);
    }
}
