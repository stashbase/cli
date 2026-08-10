//! Read-only explanation of an agent profile's HTTP capability policy.

use std::collections::HashSet;

use anyhow::{bail, Result};

use crate::{
    cmd::agent::{AgentExplainCommand, AgentProfileSource},
    config::config,
    handlers::{
        agent_validate::ensure_profile_is_valid_for_run,
        run::proxy::{
            configured_host_matches, evaluate_secret_authorization, SecretAuthorizationDecision,
            SecretHttpPolicy,
        },
    },
    models::config::Config,
};

/// Evaluates profile policy only. It never loads a secret, starts a proxy, or
/// opens a network connection.
pub fn handle_agent_explain_command(
    command: AgentExplainCommand,
    profile_config: &Config,
    silent: bool,
) -> Result<()> {
    if command.host.trim().is_empty() || command.host.trim() != command.host {
        bail!("--host must be a non-empty hostname without surrounding whitespace");
    }
    if hyper::Method::from_bytes(command.method.as_bytes()).is_err() {
        bail!("--method must be a valid HTTP method");
    }
    if !command.path.starts_with('/') {
        bail!("--path must begin with '/'");
    }

    let global_profile = profile_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let (profile, source) = match command.profile_source {
        AgentProfileSource::Global => (global_profile, "user-level config"),
        AgentProfileSource::Directory => (
            config::get_directory_agent_profile(&command.profile)?,
            "./stashbase-agent.toml",
        ),
        AgentProfileSource::Auto => {
            let directory_profile = config::get_directory_agent_profile(&command.profile)?;
            if directory_profile.is_some() {
                (directory_profile, "./stashbase-agent.toml")
            } else {
                (global_profile, "user-level config")
            }
        }
    };
    let Some(profile) = profile else {
        bail!(
            "Agent profile '{}' was not found in the {} config.",
            command.profile,
            source_label(command.profile_source)
        );
    };
    ensure_profile_is_valid_for_run(&profile)?;

    let host = command.host.trim_end_matches('.').to_ascii_lowercase();
    let method = command.method.to_ascii_uppercase();
    let path = command.path.split('?').next().unwrap_or(&command.path);

    if !silent {
        println!();
    }
    println!("Agent policy explanation: `{}`", command.profile);
    println!("Profile source: {source}");
    println!("Request: {method} https://{host}{path}");

    if profile.deny_hosts.as_deref().is_some_and(|hosts| {
        hosts
            .iter()
            .any(|denied| denied == "*" || configured_host_matches(denied, &host))
    }) {
        println!("Connection: denied by deny_hosts");
        return Ok(());
    }

    if let Some(egress_hosts) = &profile.egress_hosts {
        if !egress_hosts
            .iter()
            .any(|allowed| allowed == "*" || configured_host_matches(allowed, &host))
        {
            println!("Connection: denied by egress_hosts");
            return Ok(());
        }
        println!("Connection: allowed by egress_hosts");
    } else {
        println!("Connection: no egress_hosts restriction configured");
    }

    if profile.secrets.is_empty() {
        println!("Credentials: no Stashbase-managed secrets are configured");
        return Ok(());
    }

    for (name, secret) in &profile.secrets {
        let secret_policy = if secret.rules.is_empty() {
            SecretHttpPolicy::LegacyHosts(secret.hosts.iter().cloned().collect::<HashSet<_>>())
        } else {
            SecretHttpPolicy::Rules(secret.rules.clone())
        };
        let outcome = match evaluate_secret_authorization(&secret_policy, &host, &method, path) {
            SecretAuthorizationDecision::AllowedLegacyHost => {
                "eligible for injection (legacy hosts match)"
            }
            SecretAuthorizationDecision::AllowedRule => {
                "eligible for injection (allow rule matches)"
            }
            SecretAuthorizationDecision::DeniedLegacyHost => {
                "not eligible (legacy hosts do not match)"
            }
            SecretAuthorizationDecision::DeniedRule => "not eligible (a deny rule matches)",
            SecretAuthorizationDecision::NoMatchingAllowRule => {
                "not eligible (no allow rule matches)"
            }
        };
        println!("Credential {name}: {outcome}");
    }
    Ok(())
}

fn source_label(source: AgentProfileSource) -> &'static str {
    match source {
        AgentProfileSource::Global => "global",
        AgentProfileSource::Directory => "directory",
        AgentProfileSource::Auto => "global or directory",
    }
}
