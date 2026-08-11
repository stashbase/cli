//! Read-only explanation of an agent profile's HTTP capability policy.

use std::collections::HashSet;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::{
    cmd::agent::{AgentExplainCommand, AgentProfileSource},
    config::config,
    handlers::{
        agent_policy::{
            configured_host_matches, evaluate_secret_authorization, SecretAuthorizationDecision,
            SecretHttpPolicy,
        },
        agent_validate::ensure_profile_is_valid_for_run,
    },
    models::config::Config,
    utils::output::get_formatted_json_string,
};

/// Evaluates profile policy only. It never loads a secret, starts a proxy, or
/// opens a network connection.
pub fn handle_agent_explain_command(
    command: AgentExplainCommand,
    profile_config: &Config,
    silent: bool,
    json: bool,
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

    let mut report = ExplainReport {
        profile: command.profile,
        source: source.to_owned(),
        request: ExplainRequest {
            host,
            method,
            path: path.to_owned(),
        },
        connection: ConnectionDecision::NoEgressRestriction,
        credentials: Vec::new(),
    };
    if profile.deny_hosts.as_deref().is_some_and(|hosts| {
        hosts
            .iter()
            .any(|denied| denied == "*" || configured_host_matches(denied, &report.request.host))
    }) {
        report.connection = ConnectionDecision::DeniedByDenyHosts;
    } else if let Some(egress_hosts) = &profile.egress_hosts {
        report.connection = if egress_hosts
            .iter()
            .any(|allowed| allowed == "*" || configured_host_matches(allowed, &report.request.host))
        {
            ConnectionDecision::AllowedByEgressHosts
        } else {
            ConnectionDecision::DeniedByEgressHosts
        };
    }

    if matches!(
        report.connection,
        ConnectionDecision::AllowedByEgressHosts | ConnectionDecision::NoEgressRestriction
    ) {
        for (name, secret) in &profile.secrets {
            let secret_policy = if secret.rules.is_empty() {
                SecretHttpPolicy::LegacyHosts(secret.hosts.iter().cloned().collect::<HashSet<_>>())
            } else {
                SecretHttpPolicy::Rules(secret.rules.clone())
            };
            report.credentials.push(ExplainCredential {
                name: name.clone(),
                decision: evaluate_secret_authorization(
                    &secret_policy,
                    &report.request.host,
                    &report.request.method,
                    &report.request.path,
                )
                .into(),
            });
        }
    }

    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        print_human_report(&report, silent);
    }
    Ok(())
}

#[derive(Serialize)]
struct ExplainReport {
    profile: String,
    source: String,
    request: ExplainRequest,
    connection: ConnectionDecision,
    credentials: Vec<ExplainCredential>,
}

#[derive(Serialize)]
struct ExplainRequest {
    host: String,
    method: String,
    path: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConnectionDecision {
    DeniedByDenyHosts,
    DeniedByEgressHosts,
    AllowedByEgressHosts,
    NoEgressRestriction,
}

#[derive(Serialize)]
struct ExplainCredential {
    name: String,
    decision: ExplainCredentialDecision,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ExplainCredentialDecision {
    EligibleLegacyHost,
    EligibleRule,
    DeniedLegacyHost,
    DeniedRule,
    NoMatchingAllowRule,
}

impl From<SecretAuthorizationDecision> for ExplainCredentialDecision {
    fn from(value: SecretAuthorizationDecision) -> Self {
        match value {
            SecretAuthorizationDecision::AllowedLegacyHost => Self::EligibleLegacyHost,
            SecretAuthorizationDecision::AllowedRule => Self::EligibleRule,
            SecretAuthorizationDecision::DeniedLegacyHost => Self::DeniedLegacyHost,
            SecretAuthorizationDecision::DeniedRule => Self::DeniedRule,
            SecretAuthorizationDecision::NoMatchingAllowRule => Self::NoMatchingAllowRule,
        }
    }
}

fn print_human_report(report: &ExplainReport, silent: bool) {
    if !silent {
        println!();
    }
    println!("Agent policy explanation: `{}`", report.profile);
    println!("Profile source: {}", report.source);
    println!(
        "Request: {} https://{}{}",
        report.request.method, report.request.host, report.request.path
    );
    match report.connection {
        ConnectionDecision::DeniedByDenyHosts => println!("Connection: denied by deny_hosts"),
        ConnectionDecision::DeniedByEgressHosts => println!("Connection: denied by egress_hosts"),
        ConnectionDecision::AllowedByEgressHosts => println!("Connection: allowed by egress_hosts"),
        ConnectionDecision::NoEgressRestriction => {
            println!("Connection: no egress_hosts restriction configured")
        }
    }
    if report.credentials.is_empty()
        && matches!(
            report.connection,
            ConnectionDecision::AllowedByEgressHosts | ConnectionDecision::NoEgressRestriction
        )
    {
        println!("Credentials: no Stashbase-managed secrets are configured");
    }
    for credential in &report.credentials {
        let outcome = match credential.decision {
            ExplainCredentialDecision::EligibleLegacyHost => {
                "eligible for injection (legacy hosts match)"
            }
            ExplainCredentialDecision::EligibleRule => {
                "eligible for injection (allow rule matches)"
            }
            ExplainCredentialDecision::DeniedLegacyHost => {
                "not eligible (legacy hosts do not match)"
            }
            ExplainCredentialDecision::DeniedRule => "not eligible (a deny rule matches)",
            ExplainCredentialDecision::NoMatchingAllowRule => {
                "not eligible (no allow rule matches)"
            }
        };
        println!("Credential {}: {outcome}", credential.name);
    }
}

fn source_label(source: AgentProfileSource) -> &'static str {
    match source {
        AgentProfileSource::Global => "global",
        AgentProfileSource::Directory => "directory",
        AgentProfileSource::Auto => "global or directory",
    }
}
