//! Read-only explanation of an agent profile's HTTP capability policy.

use std::collections::HashSet;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::{
    cmd::agent::{AgentExplainCommand, AgentProfileSource},
    config::config,
    handlers::{
        agent::policy::{
            configured_host_matches, evaluate_secret_authorization, matching_rule_indices,
            normalize_request_path, SecretAuthorizationDecision, SecretHttpPolicy,
        },
        agent::validate::{ensure_profile_is_valid_for_run, uses_live_pattern_matching},
        run::{fs_rules::is_pattern_entry, subprocess::filesystem_backend_for_policy},
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
        connection: ConnectionDecision::DeniedByEgressHosts,
        egress_hosts_configured: profile.egress_hosts.is_some(),
        credentials: Vec::new(),
        reachable_hosts: reachable_hosts(&profile),
        filesystem_backend: filesystem_backend_for_policy(
            &profile.filesystem.deny_read,
            &profile.filesystem.deny_write,
        ),
        filesystem_patterns: filesystem_patterns(&profile),
    };
    let denied_by_deny_hosts = host_listed(profile.deny_hosts.as_deref(), &report.request.host);
    if !denied_by_deny_hosts {
        for (name, secret) in profile
            .secrets
            .bindings
            .iter()
            .chain(profile.personal_credentials.iter())
        {
            let secret_policy = binding_policy(secret);
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
    report
        .credentials
        .sort_by(|left, right| left.name.cmp(&right.name));
    report.connection = connection_decision(&profile, &report.request.host, &report.credentials);

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

/// Mirrors the proxy: deny_hosts wins, egress_hosts admits any request, and a
/// credential's own hosts admit only requests carrying that credential.
fn connection_decision(
    profile: &crate::models::agent::AgentProfile,
    host: &str,
    credentials: &[ExplainCredential],
) -> ConnectionDecision {
    if host_listed(profile.deny_hosts.as_deref(), host) {
        ConnectionDecision::DeniedByDenyHosts
    } else if host_listed(profile.egress_hosts.as_deref(), host) {
        ConnectionDecision::AllowedByEgressHosts
    } else if credentials.iter().any(|credential| {
        matches!(
            credential.decision,
            ExplainCredentialDecision::EligibleLegacyHost | ExplainCredentialDecision::EligibleRule
        )
    }) {
        ConnectionDecision::AllowedWithCredential
    } else {
        ConnectionDecision::DeniedByEgressHosts
    }
}

fn host_listed(hosts: Option<&[String]>, host: &str) -> bool {
    hosts.is_some_and(|hosts| {
        hosts
            .iter()
            .any(|listed| listed == "*" || configured_host_matches(listed, host))
    })
}

fn binding_policy(secret: &crate::models::agent::AgentBindingProfile) -> SecretHttpPolicy {
    if secret.rules.is_empty() {
        SecretHttpPolicy::LegacyHosts(secret.hosts.iter().cloned().collect::<HashSet<_>>())
    } else {
        SecretHttpPolicy::Rules(secret.rules.clone())
    }
}

/// Every destination the profile can reach, independent of the evaluated
/// request: `egress_hosts` alone does not list credential destinations.
fn reachable_hosts(profile: &crate::models::agent::AgentProfile) -> ExplainReachableHosts {
    let mut credentials = profile
        .secrets
        .bindings
        .iter()
        .chain(profile.personal_credentials.iter())
        .map(|(name, secret)| {
            let mut hosts = match binding_policy(secret) {
                SecretHttpPolicy::LegacyHosts(hosts) => hosts.into_iter().collect::<Vec<_>>(),
                SecretHttpPolicy::Rules(rules) => rules
                    .into_iter()
                    .filter(|rule| rule.effect == crate::models::agent::AgentHttpRuleEffect::Allow)
                    .flat_map(|rule| rule.hosts)
                    .collect(),
            };
            hosts.sort();
            hosts.dedup();
            ExplainCredentialHosts {
                name: name.clone(),
                hosts,
            }
        })
        .collect::<Vec<_>>();
    credentials.sort_by(|left, right| left.name.cmp(&right.name));
    ExplainReachableHosts {
        egress: profile.egress_hosts.clone().unwrap_or_default(),
        credentials,
        denied: profile.deny_hosts.clone().unwrap_or_default(),
    }
}

#[derive(Serialize)]
struct ExplainReport {
    profile: String,
    source: String,
    request: ExplainRequest,
    connection: ConnectionDecision,
    egress_hosts_configured: bool,
    credentials: Vec<ExplainCredential>,
    reachable_hosts: ExplainReachableHosts,
    filesystem_backend: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    filesystem_patterns: Vec<ExplainFilesystemPattern>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verbose: Option<ExplainVerboseRequest>,
}

#[derive(Serialize)]
struct ExplainFilesystemPattern {
    pattern: String,
    operation: &'static str,
    /// `live` when the backend matches on every access, `launch_snapshot`
    /// when matches are expanded to concrete paths as the run starts.
    matching: &'static str,
}

fn filesystem_patterns(
    profile: &crate::models::agent::AgentProfile,
) -> Vec<ExplainFilesystemPattern> {
    let matching = if uses_live_pattern_matching(profile) {
        "live"
    } else {
        "launch_snapshot"
    };
    [
        ("read", &profile.filesystem.deny_read),
        ("write", &profile.filesystem.deny_write),
    ]
    .into_iter()
    .flat_map(|(operation, entries)| {
        entries
            .iter()
            .filter(|entry| is_pattern_entry(entry))
            .map(move |entry| ExplainFilesystemPattern {
                pattern: entry.trim().to_owned(),
                operation,
                matching,
            })
    })
    .collect()
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
    /// Not in `egress_hosts`, and no credential is eligible for this request.
    DeniedByEgressHosts,
    AllowedByEgressHosts,
    /// Not in `egress_hosts`; allowed only when the request carries an
    /// eligible credential.
    AllowedWithCredential,
}

#[derive(Serialize)]
struct ExplainReachableHosts {
    /// Reachable by any request.
    egress: Vec<String>,
    /// Reachable only by requests carrying that credential.
    credentials: Vec<ExplainCredentialHosts>,
    denied: Vec<String>,
}

#[derive(Serialize)]
struct ExplainCredentialHosts {
    name: String,
    hosts: Vec<String>,
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
    for pattern in &report.filesystem_patterns {
        println!(
            "  deny {} pattern `{}`: {}",
            pattern.operation,
            pattern.pattern,
            match pattern.matching {
                "live" => "matched on every access",
                _ => "expanded when the run starts (files created later are not covered)",
            }
        );
    }
    println!(
        "Request: {} https://{}{}",
        report.request.method, report.request.host, report.request.path
    );
    let not_egress = if report.egress_hosts_configured {
        "not in egress_hosts"
    } else {
        "no egress_hosts configured"
    };
    match report.connection {
        ConnectionDecision::DeniedByDenyHosts => println!("Connection: denied by deny_hosts"),
        ConnectionDecision::DeniedByEgressHosts => println!(
            "Connection: denied ({not_egress}, and no credential is eligible for this request)"
        ),
        ConnectionDecision::AllowedByEgressHosts => println!("Connection: allowed by egress_hosts"),
        ConnectionDecision::AllowedWithCredential => {
            println!("Connection: allowed only with an eligible credential below ({not_egress})")
        }
    }
    if let Some(verbose) = &report.verbose {
        println!("Normalized path: {}", verbose.normalized_path);
    }
    if report.credentials.is_empty()
        && !matches!(report.connection, ConnectionDecision::DeniedByDenyHosts)
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

    let reachable = &report.reachable_hosts;
    println!("Reachable hosts:");
    if reachable.egress.is_empty() {
        let reason = if report.egress_hosts_configured {
            "egress_hosts is empty"
        } else {
            "no egress_hosts configured"
        };
        println!("  any request: none ({reason})");
    } else {
        println!("  any request: {}", reachable.egress.join(", "));
    }
    for credential in &reachable.credentials {
        let hosts = if credential.hosts.is_empty() {
            "none".to_owned()
        } else {
            credential.hosts.join(", ")
        };
        println!("  with {}: {hosts}", credential.name);
    }
    if !reachable.denied.is_empty() {
        println!("  denied: {}", reachable.denied.join(", "));
    }
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
    use super::*;
    use crate::models::agent::AgentProfile;

    fn profile(egress: &str) -> AgentProfile {
        toml::from_str(&format!(
            r#"
{egress}
deny_hosts = ["api.stashbase.dev"]

[secrets.CONTEXT7_API_KEY]

[[secrets.CONTEXT7_API_KEY.rules]]
effect = "allow"
hosts = ["mcp.context7.com"]
methods = ["POST"]
paths = ["/mcp"]

[[secrets.CONTEXT7_API_KEY.rules]]
effect = "deny"
hosts = ["admin.context7.com"]
methods = ["GET"]
paths = ["*"]
"#
        ))
        .unwrap()
    }

    fn credential(decision: ExplainCredentialDecision) -> ExplainCredential {
        ExplainCredential {
            name: "CONTEXT7_API_KEY".to_owned(),
            decision,
            matched_rule: None,
        }
    }

    #[test]
    fn a_credential_host_outside_egress_is_allowed_only_with_that_credential() {
        let profile = profile(r#"egress_hosts = ["chatgpt.com"]"#);
        let eligible = [credential(ExplainCredentialDecision::EligibleRule)];
        let ineligible = [credential(ExplainCredentialDecision::NoMatchingAllowRule)];

        assert!(matches!(
            connection_decision(&profile, "mcp.context7.com", &eligible),
            ConnectionDecision::AllowedWithCredential
        ));
        assert!(matches!(
            connection_decision(&profile, "mcp.context7.com", &ineligible),
            ConnectionDecision::DeniedByEgressHosts
        ));
        assert!(matches!(
            connection_decision(&profile, "chatgpt.com", &ineligible),
            ConnectionDecision::AllowedByEgressHosts
        ));
        assert!(matches!(
            connection_decision(&profile, "api.stashbase.dev", &eligible),
            ConnectionDecision::DeniedByDenyHosts
        ));
    }

    #[test]
    fn without_egress_hosts_only_credential_requests_are_allowed() {
        let profile = profile("");
        assert!(matches!(
            connection_decision(
                &profile,
                "mcp.context7.com",
                &[credential(ExplainCredentialDecision::EligibleRule)]
            ),
            ConnectionDecision::AllowedWithCredential
        ));
        assert!(matches!(
            connection_decision(&profile, "example.com", &[]),
            ConnectionDecision::DeniedByEgressHosts
        ));
    }

    #[test]
    fn reachable_hosts_list_credential_allow_destinations_separately_from_egress() {
        let reachable = reachable_hosts(&profile(r#"egress_hosts = ["chatgpt.com"]"#));

        assert_eq!(reachable.egress, vec!["chatgpt.com"]);
        assert_eq!(reachable.credentials.len(), 1);
        assert_eq!(reachable.credentials[0].name, "CONTEXT7_API_KEY");
        // Deny rules never make a host reachable.
        assert_eq!(reachable.credentials[0].hosts, vec!["mcp.context7.com"]);
        assert_eq!(reachable.denied, vec!["api.stashbase.dev"]);
    }
}
