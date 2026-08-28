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

    let (tests, test_source) = test_cases_for_profile(&profile, command.test_file.as_deref())?;
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
                "{marker} {}: expected {}, got {} ({})",
                result.name, result.expected, result.actual, result.reason
            );
        }
        println!("{} passed, {} failed", report.passed, report.failed);
    }
    Ok(failed)
}

fn test_cases_for_profile(
    profile: &AgentProfile,
    test_file: Option<&std::path::Path>,
) -> Result<(Vec<AgentPolicyTestCase>, String)> {
    if let Some(test_path) = test_file {
        return Ok((load_test_file(test_path)?, test_path.display().to_string()));
    }
    if !profile.policy_tests.is_empty() {
        return Ok((
            profile.policy_tests.clone(),
            "embedded profile cases".to_owned(),
        ));
    }
    let test_path = default_test_file();
    Ok((load_test_file(&test_path)?, test_path.display().to_string()))
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
    let Some(secret) = profile
        .secrets
        .bindings
        .get(&test.secret)
        .or_else(|| profile.personal_credentials.get(&test.secret))
    else {
        bail!(
            "Policy test '{name}' refers to unknown secret or credential binding '{}'.",
            test.secret
        );
    };

    let denied_by_global_hosts = profile.deny_hosts.as_deref().is_some_and(|hosts| {
        hosts
            .iter()
            .any(|denied| denied == "*" || configured_host_matches(denied, &host))
    });
    let denied_by_egress_hosts = !denied_by_global_hosts
        && profile.egress_hosts.as_deref().is_some_and(|hosts| {
            hosts
                .iter()
                .all(|allowed| allowed != "*" && !configured_host_matches(allowed, &host))
        });
    let policy = if secret.rules.is_empty() {
        SecretHttpPolicy::LegacyHosts(secret.hosts.iter().cloned().collect::<HashSet<_>>())
    } else {
        SecretHttpPolicy::Rules(secret.rules.clone())
    };
    let path = test.path.split('?').next().unwrap_or(&test.path);
    let credential_decision = evaluate_secret_authorization(&policy, &host, &test.method, path);
    let (actual, reason) = if denied_by_global_hosts {
        (
            AgentPolicyTestExpectation::Deny,
            PolicyTestReason::DeniedByDenyHosts,
        )
    } else if denied_by_egress_hosts {
        (
            AgentPolicyTestExpectation::Deny,
            PolicyTestReason::DeniedByEgressHosts,
        )
    } else {
        match credential_decision {
            SecretAuthorizationDecision::AllowedLegacyHost => (
                AgentPolicyTestExpectation::Allow,
                PolicyTestReason::AllowedLegacyHost,
            ),
            SecretAuthorizationDecision::AllowedRule => (
                AgentPolicyTestExpectation::Allow,
                PolicyTestReason::AllowedRule,
            ),
            SecretAuthorizationDecision::DeniedLegacyHost => (
                AgentPolicyTestExpectation::Deny,
                PolicyTestReason::DeniedLegacyHost,
            ),
            SecretAuthorizationDecision::DeniedRule => (
                AgentPolicyTestExpectation::Deny,
                PolicyTestReason::DeniedRule,
            ),
            SecretAuthorizationDecision::NoMatchingAllowRule => (
                AgentPolicyTestExpectation::Deny,
                PolicyTestReason::NoMatchingAllowRule,
            ),
        }
    };
    Ok(PolicyTestResult {
        name,
        secret: test.secret.clone(),
        method: test.method.to_ascii_uppercase(),
        host,
        path: path.to_owned(),
        expected: test.expect,
        actual,
        reason,
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
    reason: PolicyTestReason,
    passed: bool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum PolicyTestReason {
    DeniedByDenyHosts,
    DeniedByEgressHosts,
    AllowedLegacyHost,
    AllowedRule,
    DeniedLegacyHost,
    DeniedRule,
    NoMatchingAllowRule,
}

impl std::fmt::Display for PolicyTestReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::DeniedByDenyHosts => "connection denied by deny_hosts",
            Self::DeniedByEgressHosts => "connection denied by egress_hosts",
            Self::AllowedLegacyHost => "legacy credential host matches",
            Self::AllowedRule => "credential allow rule matches",
            Self::DeniedLegacyHost => "legacy credential hosts do not match",
            Self::DeniedRule => "credential deny rule matches",
            Self::NoMatchingAllowRule => "no credential allow rule matches",
        })
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
    use std::collections::HashMap;

    use crate::models::agent::{
        AgentBindingProfile, AgentHttpRule, AgentHttpRuleEffect, AgentPolicyTestExpectation,
    };
    use uuid::Uuid;

    use super::*;

    fn profile() -> AgentProfile {
        AgentProfile {
            file: None,
            egress_hosts: Some(vec!["api.github.com".to_owned()]),
            deny_hosts: None,
            commands: Default::default(),
            filesystem: Default::default(),
            secrets: HashMap::from([(
                "GITHUB_TOKEN".to_owned(),
                AgentBindingProfile {
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
            )])
            .into(),
            personal_credentials: HashMap::new(),
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
        assert!(matches!(allow.reason, PolicyTestReason::AllowedRule));

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
        assert!(matches!(deny.reason, PolicyTestReason::NoMatchingAllowRule));
    }

    #[test]
    fn reports_the_global_connection_denial_layer() {
        let mut profile = profile();
        profile.deny_hosts = Some(vec!["api.github.com".to_owned()]);
        let result = evaluate_case(
            &profile,
            &AgentPolicyTestCase {
                name: None,
                secret: "GITHUB_TOKEN".to_owned(),
                method: "GET".to_owned(),
                host: "api.github.com".to_owned(),
                path: "/user".to_owned(),
                expect: AgentPolicyTestExpectation::Deny,
            },
            1,
        )
        .unwrap();
        assert!(matches!(result.reason, PolicyTestReason::DeniedByDenyHosts));
    }

    #[test]
    fn reports_egress_legacy_and_explicit_rule_decisions() {
        let mut egress_denied = profile();
        egress_denied.egress_hosts = Some(vec!["registry.npmjs.org".to_owned()]);
        let result = evaluate_case(
            &egress_denied,
            &AgentPolicyTestCase {
                name: None,
                secret: "GITHUB_TOKEN".to_owned(),
                method: "GET".to_owned(),
                host: "api.github.com".to_owned(),
                path: "/user".to_owned(),
                expect: AgentPolicyTestExpectation::Deny,
            },
            1,
        )
        .unwrap();
        assert!(matches!(
            result.reason,
            PolicyTestReason::DeniedByEgressHosts
        ));

        let mut legacy = profile();
        legacy
            .secrets
            .bindings
            .get_mut("GITHUB_TOKEN")
            .unwrap()
            .rules
            .clear();
        legacy
            .secrets
            .bindings
            .get_mut("GITHUB_TOKEN")
            .unwrap()
            .hosts = vec!["api.github.com".to_owned()];
        let result = evaluate_case(
            &legacy,
            &AgentPolicyTestCase {
                name: None,
                secret: "GITHUB_TOKEN".to_owned(),
                method: "DELETE".to_owned(),
                host: "api.github.com".to_owned(),
                path: "/anything".to_owned(),
                expect: AgentPolicyTestExpectation::Allow,
            },
            2,
        )
        .unwrap();
        assert!(matches!(result.reason, PolicyTestReason::AllowedLegacyHost));

        let mut explicit_deny = profile();
        explicit_deny
            .secrets
            .bindings
            .get_mut("GITHUB_TOKEN")
            .unwrap()
            .rules
            .push(AgentHttpRule {
                effect: AgentHttpRuleEffect::Deny,
                hosts: vec!["api.github.com".to_owned()],
                methods: vec!["GET".to_owned()],
                paths: vec!["/user".to_owned()],
            });
        let result = evaluate_case(
            &explicit_deny,
            &AgentPolicyTestCase {
                name: None,
                secret: "GITHUB_TOKEN".to_owned(),
                method: "GET".to_owned(),
                host: "api.github.com".to_owned(),
                path: "/user".to_owned(),
                expect: AgentPolicyTestExpectation::Deny,
            },
            3,
        )
        .unwrap();
        assert!(matches!(result.reason, PolicyTestReason::DeniedRule));
    }

    #[test]
    fn explicit_test_file_overrides_embedded_policy_tests() {
        let mut profile = profile();
        profile.policy_tests = vec![AgentPolicyTestCase {
            name: Some("embedded".to_owned()),
            secret: "GITHUB_TOKEN".to_owned(),
            method: "GET".to_owned(),
            host: "api.github.com".to_owned(),
            path: "/user".to_owned(),
            expect: AgentPolicyTestExpectation::Allow,
        }];
        let path =
            std::env::temp_dir().join(format!("stashbase-policy-test-{}.toml", Uuid::new_v4()));
        fs::write(
            &path,
            r#"
                [[tests]]
                name = "explicit file"
                secret = "GITHUB_TOKEN"
                method = "POST"
                host = "api.github.com"
                path = "/user"
                expect = "deny"
            "#,
        )
        .unwrap();

        let (tests, source) = test_cases_for_profile(&profile, Some(&path)).unwrap();
        assert_eq!(tests[0].name.as_deref(), Some("explicit file"));
        assert_eq!(source, path.display().to_string());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn embedded_policy_tests_are_used_without_an_explicit_file() {
        let mut profile = profile();
        profile.policy_tests = vec![AgentPolicyTestCase {
            name: Some("embedded".to_owned()),
            secret: "GITHUB_TOKEN".to_owned(),
            method: "GET".to_owned(),
            host: "api.github.com".to_owned(),
            path: "/user".to_owned(),
            expect: AgentPolicyTestExpectation::Allow,
        }];

        let (tests, source) = test_cases_for_profile(&profile, None).unwrap();
        assert_eq!(tests[0].name.as_deref(), Some("embedded"));
        assert_eq!(source, "embedded profile cases");
    }
}
