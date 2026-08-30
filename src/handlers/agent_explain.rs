//! Read-only explanation of an agent profile's HTTP capability policy.

use std::collections::HashSet;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::{
    cmd::agent::{AgentExplainCommand, AgentProfileSource, CommandExplainCommand},
    config::config,
    handlers::{
        agent_policy::{
            configured_host_matches, evaluate_secret_authorization, matching_rule_indices,
            normalize_request_path, SecretAuthorizationDecision, SecretHttpPolicy,
        },
        agent_validate::ensure_profile_is_valid_for_run,
        run::subprocess::filesystem_backend_for_policy,
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

    let explicit_profile = command
        .policy_file
        .as_deref()
        .map(config::get_explicit_agent_profile)
        .transpose()?;
    let global_profile = profile_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let (profile, source) = if let Some(profile) = explicit_profile {
        (Some(profile.profile), profile.source)
    } else {
        match command.profile_source {
            AgentProfileSource::Global => (global_profile, "user-level config".to_owned()),
            AgentProfileSource::Directory => {
                match config::get_directory_agent_profile(&command.profile)? {
                    Some(profile) => (Some(profile.profile), profile.source),
                    None => (None, "directory config".to_owned()),
                }
            }
            AgentProfileSource::Auto => {
                let directory_profile = config::get_directory_agent_profile(&command.profile)?;
                if let Some(profile) = directory_profile {
                    (Some(profile.profile), profile.source)
                } else {
                    (global_profile, "user-level config".to_owned())
                }
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
    let normalized_path = normalize_request_path(path);

    let mut report = ExplainReport {
        profile: command.profile,
        source,
        request: ExplainRequest {
            host,
            method,
            path: path.to_owned(),
        },
        verbose: command.verbose.then(|| ExplainVerboseRequest {
            normalized_path: normalized_path.clone(),
        }),
        connection: ConnectionDecision::NoEgressRestriction,
        credentials: Vec::new(),
        filesystem_backend: filesystem_backend_for_policy(
            &profile.filesystem.deny_read,
            &profile.filesystem.deny_write,
        ),
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
        for (name, secret) in profile
            .secrets
            .bindings
            .iter()
            .chain(profile.personal_credentials.iter())
        {
            let secret_policy = if secret.rules.is_empty() {
                SecretHttpPolicy::LegacyHosts(secret.hosts.iter().cloned().collect::<HashSet<_>>())
            } else {
                SecretHttpPolicy::Rules(secret.rules.clone())
            };
            let decision = evaluate_secret_authorization(
                &secret_policy,
                &report.request.host,
                &report.request.method,
                &report.request.path,
            );
            let matched_rule = command
                .verbose
                .then(|| {
                    let effect = match decision {
                        SecretAuthorizationDecision::DeniedRule => {
                            Some(crate::models::agent::AgentHttpRuleEffect::Deny)
                        }
                        SecretAuthorizationDecision::AllowedRule => {
                            Some(crate::models::agent::AgentHttpRuleEffect::Allow)
                        }
                        _ => None,
                    }?;
                    matching_rule_indices(
                        &secret_policy,
                        &report.request.host,
                        &report.request.method,
                        &report.request.path,
                        effect.clone(),
                    )
                    .into_iter()
                    .next()
                    .map(|index| ExplainMatchedRule { effect, index })
                })
                .flatten();
            report.credentials.push(ExplainCredential {
                name: name.clone(),
                decision: decision.into(),
                matched_rule,
            });
        }
    }

    if !silent {
        println!();
    }
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        print_human_report(&report);
    }
    Ok(())
}

/// Explains local command enforcement without starting an agent or proxy.
pub fn handle_agent_command_explain_command(
    command: CommandExplainCommand,
    profile_config: &Config,
    silent: bool,
    json: bool,
) -> Result<()> {
    let executable = command.command.trim();
    if executable.is_empty() || executable != command.command || executable.contains(['/', '\\']) {
        bail!("--command must be a plain executable name without surrounding whitespace or path separators");
    }
    let explicit_profile = command
        .policy_file
        .as_deref()
        .map(config::get_explicit_agent_profile)
        .transpose()?;
    let global_profile = profile_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let (profile, source) = if let Some(profile) = explicit_profile {
        (Some(profile.profile), profile.source)
    } else {
        match command.profile_source {
            AgentProfileSource::Global => (global_profile, "user-level config".to_owned()),
            AgentProfileSource::Directory => {
                match config::get_directory_agent_profile(&command.profile)? {
                    Some(profile) => (Some(profile.profile), profile.source),
                    None => (None, "directory config".to_owned()),
                }
            }
            AgentProfileSource::Auto => {
                let directory_profile = config::get_directory_agent_profile(&command.profile)?;
                if let Some(profile) = directory_profile {
                    (Some(profile.profile), profile.source)
                } else {
                    (global_profile, "user-level config".to_owned())
                }
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
    let report = serde_json::json!({
        "profile": command.profile,
        "source": source,
        "command": executable,
        "decision": "allow",
        "filesystem_backend": filesystem_backend_for_policy(
            &profile.filesystem.deny_read,
            &profile.filesystem.deny_write,
        ),
    });
    if !silent {
        println!();
    }
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("Agent command policy");
        println!("Profile: {}", report["profile"]);
        println!("Command: {}", report["command"]);
        println!("Decision: {}", report["decision"]);
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
    filesystem_backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    verbose: Option<ExplainVerboseRequest>,
}

#[derive(Serialize)]
struct ExplainVerboseRequest {
    normalized_path: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_rule: Option<ExplainMatchedRule>,
}

#[derive(Serialize)]
struct ExplainMatchedRule {
    effect: crate::models::agent::AgentHttpRuleEffect,
    index: usize,
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

fn print_human_report(report: &ExplainReport) {
    println!("Agent policy explanation: `{}`", report.profile);
    println!("Profile source: {}", report.source);
    println!("Filesystem enforcement: {}", report.filesystem_backend);
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
    if let Some(verbose) = &report.verbose {
        println!("Normalized path: {}", verbose.normalized_path);
    }
    if report.credentials.is_empty()
        && matches!(
            report.connection,
            ConnectionDecision::AllowedByEgressHosts | ConnectionDecision::NoEgressRestriction
        )
    {
        println!("Credentials: no bindings are configured");
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
        if let Some(rule) = &credential.matched_rule {
            let effect = match rule.effect {
                crate::models::agent::AgentHttpRuleEffect::Allow => "allow",
                crate::models::agent::AgentHttpRuleEffect::Deny => "deny",
            };
            println!("  matched {effect} rule #{}", rule.index);
        }
    }
}

fn source_label(source: AgentProfileSource) -> &'static str {
    match source {
        AgentProfileSource::Global => "global",
        AgentProfileSource::Directory => "directory",
        AgentProfileSource::Auto => "global or directory",
    }
}
