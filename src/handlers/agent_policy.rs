//! Pure, reusable matcher for Agent Proxy credential policy.

use std::collections::HashSet;

use crate::models::agent::{AgentHttpRule, AgentHttpRuleEffect};

/// Authorization for one credential. A binding is deliberately either legacy
/// host-only policy or HTTP rules, never both.
#[derive(Debug, Clone)]
pub enum SecretHttpPolicy {
    LegacyHosts(HashSet<String>),
    Rules(Vec<AgentHttpRule>),
}

/// The credential decision for one request, without reading or exposing a
/// secret value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretAuthorizationDecision {
    AllowedLegacyHost,
    AllowedRule,
    DeniedLegacyHost,
    DeniedRule,
    NoMatchingAllowRule,
}

pub fn normalize_secret_http_policy(policy: SecretHttpPolicy) -> SecretHttpPolicy {
    match policy {
        SecretHttpPolicy::LegacyHosts(hosts) => SecretHttpPolicy::LegacyHosts(
            hosts
                .into_iter()
                .map(|host| normalize_host(&host))
                .collect(),
        ),
        SecretHttpPolicy::Rules(rules) => SecretHttpPolicy::Rules(
            rules
                .into_iter()
                .map(|mut rule| {
                    rule.hosts = rule
                        .hosts
                        .into_iter()
                        .map(|host| normalize_host(&host))
                        .collect();
                    rule.methods = rule
                        .methods
                        .into_iter()
                        .map(|method| method.trim().to_ascii_uppercase())
                        .collect();
                    rule.paths = rule
                        .paths
                        .into_iter()
                        .map(|path| normalize_path_pattern(&path))
                        .collect();
                    rule
                })
                .collect(),
        ),
    }
}

pub fn evaluate_secret_authorization(
    policy: &SecretHttpPolicy,
    host: &str,
    method: &str,
    path: &str,
) -> SecretAuthorizationDecision {
    match policy {
        SecretHttpPolicy::LegacyHosts(hosts) => {
            if hosts
                .iter()
                .any(|allowed| configured_host_matches(allowed, host))
            {
                SecretAuthorizationDecision::AllowedLegacyHost
            } else {
                SecretAuthorizationDecision::DeniedLegacyHost
            }
        }
        SecretHttpPolicy::Rules(rules) => {
            let path = normalize_request_path(path);
            let matches = |rule: &AgentHttpRule| {
                rule.hosts
                    .iter()
                    .any(|allowed| configured_host_matches(allowed, host))
                    && rule
                        .methods
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(method))
                    && rule
                        .paths
                        .iter()
                        .any(|pattern| path_matches(pattern, &path))
            };
            if rules
                .iter()
                .any(|rule| rule.effect == AgentHttpRuleEffect::Deny && matches(rule))
            {
                SecretAuthorizationDecision::DeniedRule
            } else if rules
                .iter()
                .any(|rule| rule.effect == AgentHttpRuleEffect::Allow && matches(rule))
            {
                SecretAuthorizationDecision::AllowedRule
            } else {
                SecretAuthorizationDecision::NoMatchingAllowRule
            }
        }
    }
}

pub fn host_matches(allowed: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match allowed.strip_prefix("*.") {
        Some(suffix) => host != suffix && host.ends_with(&format!(".{suffix}")),
        None => allowed == host,
    }
}

pub fn configured_host_matches(allowed: &str, host: &str) -> bool {
    host_matches(&normalize_host(allowed), host)
}

fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn normalize_path_pattern(pattern: &str) -> String {
    if pattern == "*" {
        return "*".to_owned();
    }
    normalize_path_segments(pattern)
}

fn normalize_request_path(path: &str) -> String {
    normalize_path_segments(path)
}

fn normalize_path_segments(path: &str) -> String {
    let mut segments = Vec::new();
    let path = path
        .replace("%2f", "/")
        .replace("%2F", "/")
        .replace("%5c", "/")
        .replace("%5C", "/")
        .replace('\\', "/");
    for segment in path.split('/') {
        let segment = segment.replace("%2e", ".").replace("%2E", ".");
        match segment.as_str() {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    format!("/{}", segments.join("/"))
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let mut remainder = path;
    let mut first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        if first {
            if !remainder.starts_with(part) {
                return false;
            }
            remainder = &remainder[part.len()..];
            first = false;
        } else if let Some(index) = remainder.find(part) {
            remainder = &remainder[index + part.len()..];
        } else {
            return false;
        }
    }
    pattern.ends_with('*') || remainder.is_empty()
}
