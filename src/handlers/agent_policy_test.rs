//! Local, declarative regression tests for Agent Proxy HTTP policy.

use std::{collections::HashSet, fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    cmd::agent::{AgentPolicyTestCommand, AgentProfileSource},
    config::config,
    handlers::{
        agent_policy::{
            configured_host_matches, evaluate_secret_authorization, SecretAuthorizationDecision,
            SecretHttpPolicy,
        },
        agent_validate::ensure_profile_is_valid_for_run,
    },
    models::{
        agent::{AgentPolicyTestCase, AgentPolicyTestExpectation, AgentProfile},
        config::Config,
    },
    utils::output::get_formatted_json_string,
};

const DEFAULT_TEST_FILE: &str = ".stashbase/agent-policy-tests.toml";

/// Runs policy fixtures only. It never loads a secret, starts a proxy, or opens
/// a network connection. Returns true when at least one case fails.
pub fn handle_agent_policy_test_command(
    command: AgentPolicyTestCommand,
    profile_config: &Config,
    silent: bool,
    json: bool,
) -> Result<bool> {
    let (profile, source) = load_profile(&command, profile_config)?;
    ensure_profile_is_valid_for_run(&profile)?;

    let (tests, test_source) = if let Some(test_path) = command.test_file {
        (load_test_file(&test_path)?, test_path.display().to_string())
    } else if !profile.policy_tests.is_empty() {
        (
            profile.policy_tests.clone(),
            "embedded profile cases".to_owned(),
        )
    } else {
        let test_path = default_test_file();
        (load_test_file(&test_path)?, test_path.display().to_string())
    };
    let results = tests
        .iter()
        .enumerate()
        .map(|(index, test)| evaluate_case(&profile, test, index + 1))
        .collect::<Result<Vec<_>>>()?;
    let failed = results.iter().any(|result| !result.passed);
    let report = PolicyTestReport {
        profile: command.profile,
        source,
        test_source,
        passed: results.iter().filter(|result| result.passed).count(),
        failed: results.iter().filter(|result| !result.passed).count(),
        tests: results,
    };

    if !silent {
        println!();
    }
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("Agent policy tests: `{}`", report.profile);
        println!("Profile source: {}", report.source);
        println!("Test source: {}", report.test_source);
        for result in &report.tests {
            let marker = if result.passed { "✓" } else { "✗" };
            println!(
                "{marker} {}: expected {}, got {}",
                result.name, result.expected, result.actual
            );
        }
        println!("{} passed, {} failed", report.passed, report.failed);
    }
    Ok(failed)
}

fn load_test_file(test_path: &std::path::Path) -> Result<Vec<AgentPolicyTestCase>> {
    let contents = fs::read_to_string(test_path).with_context(|| {
        format!(
            "Could not read agent policy test file '{}'.",
            test_path.display()
        )
    })?;
    let fixtures: PolicyTestFile = toml::from_str(&contents).with_context(|| {
        format!(
            "Could not parse agent policy test file '{}'.",
            test_path.display()
        )
    })?;
    if fixtures.tests.is_empty() {
        bail!(
            "Agent policy test file '{}' does not contain any [[tests]] cases.",
            test_path.display()
        );
    }
    Ok(fixtures.tests)
}

fn default_test_file() -> PathBuf {
    PathBuf::from(DEFAULT_TEST_FILE)
}

fn load_profile(
    command: &AgentPolicyTestCommand,
    profile_config: &Config,
) -> Result<(AgentProfile, String)> {
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
    let resolved = if let Some(profile) = explicit_profile {
        Some((profile.profile, profile.source))
    } else {
        match command.profile_source {
            AgentProfileSource::Global => {
                global_profile.map(|profile| (profile, "user-level config".to_owned()))
            }
            AgentProfileSource::Directory => config::get_directory_agent_profile(&command.profile)?
                .map(|profile| (profile.profile, profile.source)),
            AgentProfileSource::Auto => config::get_directory_agent_profile(&command.profile)?
                .map(|profile| (profile.profile, profile.source))
                .or_else(|| {
                    global_profile.map(|profile| (profile, "user-level config".to_owned()))
                }),
        }
    };
    resolved.ok_or_else(|| {
        anyhow::anyhow!(
            "Agent profile '{}' was not found in the {} config.",
            command.profile,
            source_label(command.profile_source)
        )
    })
}

fn evaluate_case(
    profile: &AgentProfile,
    test: &AgentPolicyTestCase,
    index: usize,
) -> Result<PolicyTestResult> {
    let name = test.name.clone().unwrap_or_else(|| format!("case {index}"));
    if test.host.trim().is_empty() || test.host.trim() != test.host {
        bail!("Policy test '{name}' has an invalid host.");
    }
    let host = test.host.trim_end_matches('.').to_ascii_lowercase();
    if hyper::Method::from_bytes(test.method.as_bytes()).is_err() {
        bail!("Policy test '{name}' has an invalid HTTP method.");
    }
    if !test.path.starts_with('/') {
        bail!("Policy test '{name}' path must begin with '/'.");
    }
    let Some(secret) = profile.secrets.get(&test.secret) else {
        bail!(
            "Policy test '{name}' refers to unknown secret binding '{}'.",
            test.secret
        );
    };

    let connection_allowed = !profile.deny_hosts.as_deref().is_some_and(|hosts| {
        hosts
            .iter()
            .any(|denied| denied == "*" || configured_host_matches(denied, &host))
    }) && profile.egress_hosts.as_deref().is_none_or(|hosts| {
        hosts
            .iter()
            .any(|allowed| allowed == "*" || configured_host_matches(allowed, &host))
    });
    let policy = if secret.rules.is_empty() {
        SecretHttpPolicy::LegacyHosts(secret.hosts.iter().cloned().collect::<HashSet<_>>())
    } else {
        SecretHttpPolicy::Rules(secret.rules.clone())
    };
    let path = test.path.split('?').next().unwrap_or(&test.path);
    let credential_allowed = matches!(
        evaluate_secret_authorization(&policy, &host, &test.method, path),
        SecretAuthorizationDecision::AllowedLegacyHost | SecretAuthorizationDecision::AllowedRule
    );
    let actual = if connection_allowed && credential_allowed {
        AgentPolicyTestExpectation::Allow
    } else {
        AgentPolicyTestExpectation::Deny
    };
    Ok(PolicyTestResult {
        name,
        secret: test.secret.clone(),
        method: test.method.to_ascii_uppercase(),
        host,
        path: path.to_owned(),
        expected: test.expect,
        actual,
        passed: actual == test.expect,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyTestFile {
    #[serde(default)]
    tests: Vec<AgentPolicyTestCase>,
}

#[derive(Serialize)]
struct PolicyTestReport {
    profile: String,
    source: String,
    test_source: String,
    passed: usize,
    failed: usize,
    tests: Vec<PolicyTestResult>,
}

#[derive(Serialize)]
struct PolicyTestResult {
    name: String,
    secret: String,
    method: String,
    host: String,
    path: String,
    expected: AgentPolicyTestExpectation,
    actual: AgentPolicyTestExpectation,
    passed: bool,
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

    use crate::models::agent::{
        AgentHttpRule, AgentHttpRuleEffect, AgentPolicyTestExpectation, AgentSecretProfile,
    };

    use super::*;

    fn profile() -> AgentProfile {
        AgentProfile {
            project: None,
            environment: None,
            file: None,
            egress_hosts: Some(vec!["api.github.com".to_owned()]),
            deny_hosts: None,
            secrets: HashMap::from([(
                "GITHUB_TOKEN".to_owned(),
                AgentSecretProfile {
                    hosts: Vec::new(),
                    rules: vec![AgentHttpRule {
                        effect: AgentHttpRuleEffect::Allow,
                        hosts: vec!["api.github.com".to_owned()],
                        methods: vec!["GET".to_owned()],
                        paths: vec!["/user".to_owned()],
                    }],
                    from: None,
                    env: None,
                    placeholder: None,
                    header: None,
                    value_template: None,
                },
            )]),
            policy_tests: Vec::new(),
        }
    }

    #[test]
    fn evaluates_allowed_and_denied_cases_without_a_network_request() {
        let allow = evaluate_case(
            &profile(),
            &AgentPolicyTestCase {
                name: None,
                secret: "GITHUB_TOKEN".to_owned(),
                method: "get".to_owned(),
                host: "api.github.com".to_owned(),
                path: "/user".to_owned(),
                expect: AgentPolicyTestExpectation::Allow,
            },
            1,
        )
        .unwrap();
        assert!(allow.passed);

        let deny = evaluate_case(
            &profile(),
            &AgentPolicyTestCase {
                name: None,
                secret: "GITHUB_TOKEN".to_owned(),
                method: "POST".to_owned(),
                host: "api.github.com".to_owned(),
                path: "/user".to_owned(),
                expect: AgentPolicyTestExpectation::Deny,
            },
            2,
        )
        .unwrap();
        assert!(deny.passed);
    }
}
