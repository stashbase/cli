//! Read-only discovery and inspection of agent profiles.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::{
    cmd::agent::{
        AgentProfileSource, AgentProfilesCommand, AgentProfilesListCommand,
        AgentProfilesShowCommand, AgentProfilesSubcommand,
    },
    config::config,
    models::{agent::AgentProfile, config::Config},
    utils::output::get_formatted_json_string,
};

pub fn handle_agent_profiles_command(
    command: AgentProfilesCommand,
    global_config: &Config,
    json: bool,
) -> Result<()> {
    match command.subcommand {
        AgentProfilesSubcommand::List(command) => handle_list(command, global_config, json),
        AgentProfilesSubcommand::Show(command) => handle_show(command, global_config, json),
    }
}

fn handle_list(
    command: AgentProfilesListCommand,
    global_config: &Config,
    json: bool,
) -> Result<()> {
    let profiles = profiles_for_source(command.profile_source, global_config)?;
    let report = profiles
        .into_iter()
        .map(|(name, (profile, source))| ProfileSummary {
            name,
            source,
            secret_bindings: profile.secrets.len(),
            egress_hosts_configured: profile.egress_hosts.is_some(),
        })
        .collect::<Vec<_>>();
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else if report.is_empty() {
        println!("No agent profiles found.");
    } else {
        println!("Available agent profiles:");
        for profile in report {
            println!(
                "- {} ({}, {} secret binding{}, {} egress_hosts)",
                profile.name,
                profile.source,
                profile.secret_bindings,
                if profile.secret_bindings == 1 {
                    ""
                } else {
                    "s"
                },
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
    let profiles = profiles_for_source(command.profile_source, global_config)?;
    let Some((profile, source)) = profiles.get(&command.profile) else {
        bail!(
            "Agent profile '{}' was not found in the {} config.",
            command.profile,
            source_label(command.profile_source)
        );
    };
    let report = ProfileDetails {
        name: command.profile,
        source: source.clone(),
        profile: profile.clone(),
    };
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("Agent profile: {}", report.name);
        println!("Profile source: {}", report.source);
        println!();
        print!("{}", toml::to_string_pretty(&report.profile)?);
    }
    Ok(())
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
    secret_bindings: usize,
    egress_hosts_configured: bool,
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
