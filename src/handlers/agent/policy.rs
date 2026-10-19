//! Pure, reusable matcher for Agent Proxy credential policy.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use serde::de::{Deserialize, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::models::agent::{AgentFieldCondition, AgentHttpRule, AgentHttpRuleEffect, AgentScalar};

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
            let matches = |rule: &AgentHttpRule| rule_matches(rule, host, method, path);
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

/// Returns one-based matching rule numbers in configured order for local
/// diagnostics. Authorization still evaluates all matching deny rules first.
pub fn matching_rule_indices(
    policy: &SecretHttpPolicy,
    host: &str,
    method: &str,
    path: &str,
    effect: AgentHttpRuleEffect,
) -> Vec<usize> {
    let SecretHttpPolicy::Rules(rules) = policy else {
        return Vec::new();
    };
    rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            (rule.effect == effect && rule_matches(rule, host, method, path)).then_some(index + 1)
        })
        .collect()
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

pub(crate) fn normalize_path_pattern(pattern: &str) -> String {
    if pattern == "*" {
        return "*".to_owned();
    }
    normalize_path_segments(pattern)
}

pub fn normalize_request_path(path: &str) -> String {
    normalize_path_segments(path)
}

fn rule_matches(rule: &AgentHttpRule, host: &str, method: &str, path: &str) -> bool {
    let path = normalize_request_path(path);
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

/// Returns the first field path whose condition does not hold against
/// `value`, or `None` when every condition holds.
pub fn first_unmet_condition<'a>(
    conditions: &'a BTreeMap<String, AgentFieldCondition>,
    value: &Value,
) -> Option<&'a str> {
    conditions
        .iter()
        .find(|(path, condition)| !condition_holds(condition, lookup(value, path)))
        .map(|(path, _)| path.as_str())
}

/// Where a dotted field path leads. `Blocked` means an intermediate segment
/// was an array or scalar, so the path is neither present nor safely absent.
enum Lookup<'a> {
    Found(&'a Value),
    Missing,
    Blocked,
}

fn lookup<'a>(value: &'a Value, path: &str) -> Lookup<'a> {
    let mut current = value;
    for segment in path.split('.') {
        match current {
            Value::Object(object) => match object.get(segment) {
                Some(next) => current = next,
                None => return Lookup::Missing,
            },
            Value::Null => return Lookup::Missing,
            _ => return Lookup::Blocked,
        }
    }
    match current {
        Value::Null => Lookup::Missing,
        found => Lookup::Found(found),
    }
}

fn condition_holds(condition: &AgentFieldCondition, field: Lookup<'_>) -> bool {
    if let AgentFieldCondition::Absent(absent) = condition {
        return absent.absent && matches!(field, Lookup::Missing);
    }
    let Lookup::Found(field) = field else {
        return false;
    };
    match condition {
        AgentFieldCondition::Equals(expected) => scalar_matches(expected, field),
        AgentFieldCondition::OneOf(allowed) => match field.as_array() {
            Some(items) => {
                !items.is_empty()
                    && items.iter().all(|item| {
                        allowed
                            .iter()
                            .any(|expected| scalar_matches(expected, item))
                    })
            }
            None => allowed
                .iter()
                .any(|expected| scalar_matches(expected, field)),
        },
        AgentFieldCondition::Range(range) => field.as_f64().is_some_and(|number| {
            range.min.is_none_or(|min| number >= min) && range.max.is_none_or(|max| number <= max)
        }),
        AgentFieldCondition::Absent(_) => false,
    }
}

fn scalar_matches(expected: &AgentScalar, field: &Value) -> bool {
    match expected {
        AgentScalar::Bool(expected) => field.as_bool() == Some(*expected),
        AgentScalar::Integer(expected) => field.as_f64() == Some(*expected as f64),
        AgentScalar::Float(expected) => field.as_f64() == Some(*expected),
        AgentScalar::String(expected) => field.as_str() == Some(expected.as_str()),
    }
}

/// Parses JSON, rejecting duplicate object keys at any depth. serde_json keeps
/// the last duplicate while an upstream parser may keep the first, so a policy
/// decision on such a body could differ from what the upstream executes.
pub fn parse_json_with_unique_keys(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice::<UniqueKeys>(bytes).ok()?;
    serde_json::from_slice(bytes).ok()
}

struct UniqueKeys;

impl<'de> Deserialize<'de> for UniqueKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueKeysVisitor)
    }
}

struct UniqueKeysVisitor;

impl<'de> Visitor<'de> for UniqueKeysVisitor {
    type Value = UniqueKeys;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<UniqueKeys, E> {
        Ok(UniqueKeys)
    }

    fn visit_i64<E>(self, _: i64) -> Result<UniqueKeys, E> {
        Ok(UniqueKeys)
    }

    fn visit_u64<E>(self, _: u64) -> Result<UniqueKeys, E> {
        Ok(UniqueKeys)
    }

    fn visit_f64<E>(self, _: f64) -> Result<UniqueKeys, E> {
        Ok(UniqueKeys)
    }

    fn visit_str<E>(self, _: &str) -> Result<UniqueKeys, E> {
        Ok(UniqueKeys)
    }

    fn visit_unit<E>(self) -> Result<UniqueKeys, E> {
        Ok(UniqueKeys)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<UniqueKeys, A::Error> {
        while seq.next_element::<UniqueKeys>()?.is_some() {}
        Ok(UniqueKeys)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<UniqueKeys, A::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(A::Error::custom("duplicate object key"));
            }
            map.next_value::<UniqueKeys>()?;
        }
        Ok(UniqueKeys)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{
        evaluate_secret_authorization, first_unmet_condition, matching_rule_indices,
        parse_json_with_unique_keys, SecretHttpPolicy,
    };
    use crate::models::agent::{AgentFieldCondition, AgentHttpRule, AgentHttpRuleEffect};

    fn conditions(toml: &str) -> BTreeMap<String, AgentFieldCondition> {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            arguments: BTreeMap<String, AgentFieldCondition>,
        }
        toml::from_str::<Wrapper>(toml).unwrap().arguments
    }

    #[test]
    fn scalar_conditions_require_the_same_json_type() {
        let rule = conditions(r#"arguments = { mode = "draft", count = 5, send = false }"#);
        assert_eq!(
            first_unmet_condition(&rule, &json!({"mode": "draft", "count": 5, "send": false})),
            None
        );
        assert_eq!(
            first_unmet_condition(
                &rule,
                &json!({"mode": "draft", "count": 5.0, "send": false})
            ),
            None
        );
        assert_eq!(
            first_unmet_condition(
                &rule,
                &json!({"mode": "draft", "count": "5", "send": false})
            ),
            Some("count")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"mode": "Draft", "count": 5, "send": false})),
            Some("mode")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"mode": "draft", "count": 5})),
            Some("send")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"mode": null, "count": 5, "send": false})),
            Some("mode")
        );
    }

    #[test]
    fn list_conditions_match_a_scalar_or_every_array_element() {
        let rule = conditions(r#"arguments = { to = ["a@acme.com", "b@acme.com"] }"#);
        assert_eq!(
            first_unmet_condition(&rule, &json!({"to": "a@acme.com"})),
            None
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"to": ["a@acme.com", "b@acme.com"]})),
            None
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"to": ["a@acme.com", "x@evil.com"]})),
            Some("to")
        );
        assert_eq!(first_unmet_condition(&rule, &json!({"to": []})), Some("to"));
        assert_eq!(
            first_unmet_condition(&rule, &json!({"to": {"0": "a@acme.com"}})),
            Some("to")
        );
    }

    #[test]
    fn range_conditions_are_inclusive_and_numeric() {
        let rule = conditions("arguments = { amount = { min = 1, max = 5000 } }");
        assert_eq!(first_unmet_condition(&rule, &json!({"amount": 1})), None);
        assert_eq!(first_unmet_condition(&rule, &json!({"amount": 5000})), None);
        assert_eq!(
            first_unmet_condition(&rule, &json!({"amount": 5000.5})),
            Some("amount")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"amount": 0})),
            Some("amount")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"amount": "100"})),
            Some("amount")
        );
        let open_ended = conditions("arguments = { amount = { max = 10 } }");
        assert_eq!(
            first_unmet_condition(&open_ended, &json!({"amount": -3})),
            None
        );
    }

    #[test]
    fn dotted_paths_walk_objects_only() {
        let rule = conditions(r#"arguments = { "destination.type" = "bank" }"#);
        assert_eq!(
            first_unmet_condition(&rule, &json!({"destination": {"type": "bank"}})),
            None
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"destination": [{"type": "bank"}]})),
            Some("destination.type")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"destination.type": "bank"})),
            Some("destination.type")
        );
    }

    #[test]
    fn absent_conditions_hold_for_missing_or_null_fields_only() {
        let rule =
            conditions(r#"arguments = { id = { absent = true }, "meta.ref" = { absent = true } }"#);
        assert_eq!(first_unmet_condition(&rule, &json!({})), None);
        assert_eq!(
            first_unmet_condition(&rule, &json!({"id": null, "meta": null})),
            None
        );
        assert_eq!(first_unmet_condition(&rule, &json!({"meta": {}})), None);
        assert_eq!(
            first_unmet_condition(&rule, &json!({"id": "x"})),
            Some("id")
        );
        assert_eq!(first_unmet_condition(&rule, &json!({"id": ""})), Some("id"));
        assert_eq!(
            first_unmet_condition(&rule, &json!({"meta": {"ref": 1}})),
            Some("meta.ref")
        );
        assert_eq!(
            first_unmet_condition(&rule, &json!({"meta": [{"ref": 1}]})),
            Some("meta.ref"),
            "a path blocked by an array is not absent"
        );
    }

    #[test]
    fn unknown_range_keys_are_rejected_when_parsing_the_profile() {
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Wrapper {
            arguments: BTreeMap<String, AgentFieldCondition>,
        }
        assert!(toml::from_str::<Wrapper>("arguments = { amount = { maximum = 5 } }").is_err());
    }

    #[test]
    fn strict_json_parser_rejects_duplicate_keys_at_any_depth() {
        assert!(parse_json_with_unique_keys(br#"{"a": 1, "b": {"c": [1, {"d": 2}]}}"#).is_some());
        assert!(parse_json_with_unique_keys(br#"{"a": 1, "a": 2}"#).is_none());
        assert!(parse_json_with_unique_keys(br#"{"a": {"b": 1, "b": 2}}"#).is_none());
        assert!(parse_json_with_unique_keys(br#"[{"b": 1, "b": 2}]"#).is_none());
        assert!(parse_json_with_unique_keys(br#"{"name": 1, "name": 2}"#).is_none());
        assert!(parse_json_with_unique_keys(b"{not json").is_none());
    }

    #[test]
    fn reports_one_based_matching_rule_numbers_with_deny_precedence() {
        let policy = SecretHttpPolicy::Rules(vec![
            AgentHttpRule {
                effect: AgentHttpRuleEffect::Allow,
                hosts: vec!["api.example.com".to_owned()],
                methods: vec!["GET".to_owned()],
                paths: vec!["/repos/*".to_owned()],
            },
            AgentHttpRule {
                effect: AgentHttpRuleEffect::Deny,
                hosts: vec!["api.example.com".to_owned()],
                methods: vec!["GET".to_owned()],
                paths: vec!["/repos/private/*".to_owned()],
            },
        ]);

        assert_eq!(
            evaluate_secret_authorization(
                &policy,
                "api.example.com",
                "GET",
                "/repos/private/../private/a"
            ),
            super::SecretAuthorizationDecision::DeniedRule
        );
        assert_eq!(
            matching_rule_indices(
                &policy,
                "api.example.com",
                "GET",
                "/repos/private/../private/a",
                AgentHttpRuleEffect::Allow,
            ),
            vec![1]
        );
        assert_eq!(
            matching_rule_indices(
                &policy,
                "api.example.com",
                "GET",
                "/repos/private/../private/a",
                AgentHttpRuleEffect::Deny,
            ),
            vec![2]
        );
    }
}
