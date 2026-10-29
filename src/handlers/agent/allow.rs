//! Turns a request the Agent Proxy denied into the narrowest profile change
//! that allows it, and summarizes a run's denials when it ends.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::{
    cmd::agent::AgentAllowCommand,
    handlers::{
        agent::policy::{
            configured_host_matches, evaluate_secret_authorization, normalize_request_path,
            normalize_secret_http_policy, SecretAuthorizationDecision, SecretHttpPolicy,
        },
        agent::validate::ensure_profile_is_valid_for_run,
        run::proxy::{read_local_proxy_audit_logs, ProxyAuditLogEvent, ProxyAuditLogFilter},
    },
    models::agent::{AgentBindingProfile, AgentProfile},
    utils::output::get_formatted_json_string,
};

/// Enough to cover any session the local audit retention keeps.
const AUDIT_EVENT_LIMIT: usize = 100_000;
const SUMMARY_GROUP_LIMIT: usize = 10;

/// The profile change that allows one denied request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProfileChange {
    /// Add `host` to top-level `egress_hosts`.
    EgressHost { host: String },
    /// Append an allow rule to `[<table>.<binding>]`.
    CredentialRule {
        table: &'static str,
        binding: String,
        host: String,
        method: String,
        path: String,
    },
    /// Add `tool` to `[mcp_servers.<server>] allow_tools`.
    McpTool { server: String, tool: String },
}

impl ProfileChange {
    fn describe(&self) -> String {
        match self {
            Self::EgressHost { host } => format!("add {host} to egress_hosts"),
            Self::CredentialRule {
                binding,
                host,
                method,
                path,
                ..
            } => format!("allow {binding} for {method} {host}{path}"),
            Self::McpTool { server, tool } => {
                format!("add {tool} to the allow_tools of MCP server {server}")
            }
        }
    }
}

/// Whether `agent allow` may be able to fix this denial. Cheap and
/// profile-free, for the end-of-run summary; `denial_change` decides.
fn may_be_fixable(event: &ProxyAuditLogEvent) -> bool {
    match event.action.as_str() {
        "host_denied" => event.binding_name.is_none() && event.destination_host.is_some(),
        "credential_rule_denied" => event.reason.is_none() && event.path.is_some(),
        "mcp_tool_denied" => {
            event.reason.as_deref() == Some("tool_not_allowed") && event.mcp_tool.is_some()
        }
        _ => false,
    }
}

/// The change that allows the denied request in `event`, or why there is
/// none that `agent allow` can make safely.
fn denial_change(
    event: &ProxyAuditLogEvent,
    profile: &AgentProfile,
) -> std::result::Result<ProfileChange, String> {
    let host = event.destination_host.clone().unwrap_or_default();
    match event.action.as_str() {
        "host_denied" if event.binding_name.is_some() => Err(format!(
            "{} has a legacy `hosts` list that does not include {host}; add the host there, \
             or replace `hosts` with `rules`",
            event.binding_name.as_deref().unwrap_or_default()
        )),
        "host_denied" if host.is_empty() => Err("the denial does not name a host".to_owned()),
        "host_denied" => {
            if host_listed(profile.deny_hosts.as_deref(), &host) {
                return Err(format!(
                    "{host} is listed in deny_hosts; remove it there if the agent needs it"
                ));
            }
            if host_listed(profile.egress_hosts.as_deref(), &host) {
                return Err(format!(
                    "{host} is already in egress_hosts (the profile changed after the run)"
                ));
            }
            Ok(ProfileChange::EgressHost { host })
        }
        "credential_rule_denied" => {
            if let Some(reason) = &event.reason {
                return Err(format!(
                    "a rule's request conditions decided this ({reason}); edit them by hand"
                ));
            }
            let binding = event
                .binding_name
                .clone()
                .ok_or("the denial does not name a credential")?;
            let method = event
                .method
                .clone()
                .ok_or("the denial does not record a method")?;
            let path = event.path.clone().ok_or(
                "the denial does not record a path (runs before this version didn't); \
                 add the rule by hand",
            )?;
            let (table, secret) = binding_entry(profile, &binding).ok_or(format!(
                "credential {binding} is not in the profile (the profile changed after the run)"
            ))?;
            if secret.rules.is_empty() {
                return Err(format!(
                    "{binding} has a legacy `hosts` list; add {host} there, or replace `hosts` \
                     with `rules`"
                ));
            }
            match credential_decision(secret, &host, &method, &path) {
                SecretAuthorizationDecision::DeniedRule => Err(format!(
                    "a deny rule on {binding} matches {method} {host}{path}; deny rules win, so \
                     change that rule if the agent needs this"
                )),
                SecretAuthorizationDecision::AllowedRule => Err(format!(
                    "{binding} already allows {method} {host}{path} (the profile changed after \
                     the run)"
                )),
                _ => Ok(ProfileChange::CredentialRule {
                    table,
                    binding,
                    host,
                    method,
                    path,
                }),
            }
        }
        "mcp_tool_denied" => {
            let tool = event
                .mcp_tool
                .clone()
                .ok_or("the denial does not name an MCP tool")?;
            match event.reason.as_deref() {
                Some("tool_not_allowed") => {}
                Some(reason) => {
                    return Err(format!(
                        "the MCP server's rules denied {tool} ({reason}); edit them by hand"
                    ))
                }
                None => return Err(format!("the denial of {tool} records no reason")),
            }
            let path = event.path.as_deref().unwrap_or_default();
            let server = profile
                .mcp_servers
                .iter()
                .find(|(_, server)| {
                    reqwest::Url::parse(&server.url).is_ok_and(|url| {
                        url.host_str() == Some(host.as_str()) && url.path() == path
                    })
                })
                .map(|(name, _)| name.clone())
                .ok_or(format!(
                    "no MCP server in the profile has the endpoint {host}{path}"
                ))?;
            Ok(ProfileChange::McpTool { server, tool })
        }
        "filesystem_denied" => Err(
            "filesystem denials come from [filesystem] deny_read/deny_write; edit those by hand"
                .to_owned(),
        ),
        action => Err(format!(
            "`{action}` denials can't be allowed from the profile automatically"
        )),
    }
}

fn host_listed(hosts: Option<&[String]>, host: &str) -> bool {
    hosts.is_some_and(|hosts| {
        hosts
            .iter()
            .any(|listed| listed == "*" || configured_host_matches(listed, host))
    })
}

fn binding_entry<'a>(
    profile: &'a AgentProfile,
    binding: &str,
) -> Option<(&'static str, &'a AgentBindingProfile)> {
    profile
        .secrets
        .bindings
        .get(binding)
        .map(|secret| ("secrets", secret))
        .or_else(|| {
            profile
                .personal_credentials
                .get(binding)
                .map(|secret| ("personal_credentials", secret))
        })
}

/// The decision the proxy makes for a request without body conditions in
/// play, with the same normalization it applies at startup.
fn credential_decision(
    secret: &AgentBindingProfile,
    host: &str,
    method: &str,
    path: &str,
) -> SecretAuthorizationDecision {
    let policy = normalize_secret_http_policy(SecretHttpPolicy::Rules(secret.rules.clone()));
    evaluate_secret_authorization(&policy, host, method, &normalize_request_path(path))
}

/// The profile text with `change` applied. Edits are line-based so comments
/// and layout survive; callers re-parse the result before writing it.
fn apply_change(content: &str, change: &ProfileChange) -> Result<String> {
    match change {
        ProfileChange::EgressHost { host } => add_egress_host(content, host),
        ProfileChange::CredentialRule {
            table,
            binding,
            host,
            method,
            path,
        } => Ok(append_credential_rule(
            content, table, binding, host, method, path,
        )?),
        ProfileChange::McpTool { server, tool } => {
            let profile = toml::from_str::<AgentProfile>(content)?;
            let mut tools = profile
                .mcp_servers
                .get(server)
                .map(|server| server.allow_tools.clone())
                .unwrap_or_default();
            tools.push(tool.clone());
            crate::handlers::agent::mcp::replace_mcp_allow_tools(content, server, &tools)
        }
    }
}

/// The part of a TOML line before a comment. Hosts and the keys we look for
/// never contain `#`, so a plain split is enough here.
fn code_part(line: &str) -> &str {
    line.split('#').next().unwrap_or(line)
}

fn add_egress_host(content: &str, host: &str) -> Result<String> {
    let quoted = serde_json::to_string(host)?;
    let mut lines = content.lines().map(str::to_owned).collect::<Vec<_>>();
    // Top-level keys must come before the first table header.
    let first_table = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let existing = (0..first_table).find(|index| {
        let code = code_part(&lines[*index]);
        code.trim_start().starts_with("egress_hosts")
            && code.split('=').next().map(str::trim) == Some("egress_hosts")
    });
    match existing {
        Some(index) if code_part(&lines[index]).contains(']') => {
            let line = &lines[index];
            let close = code_part(line)
                .rfind(']')
                .context("egress_hosts is missing its closing bracket")?;
            let open = code_part(line)
                .find('[')
                .context("egress_hosts is not an array")?;
            let inner = line[open + 1..close].trim();
            let values = if inner.is_empty() {
                quoted
            } else {
                format!("{}, {quoted}", inner.trim_end_matches(',').trim_end())
            };
            lines[index] = format!("{}{values}{}", &line[..=open], &line[close..]);
        }
        Some(index) => {
            let close = (index + 1..lines.len())
                .find(|line| code_part(&lines[*line]).trim_start().starts_with(']'))
                .context("egress_hosts is missing its closing bracket")?;
            let previous = (index + 1..close)
                .rev()
                .find(|line| !code_part(&lines[*line]).trim().is_empty());
            let indent = previous
                .map(|line| {
                    let text = &lines[line];
                    text[..text.len() - text.trim_start().len()].to_owned()
                })
                .unwrap_or_else(|| "    ".to_owned());
            if let Some(previous) = previous {
                let line = &lines[previous];
                let code = code_part(line).trim_end();
                if !code.ends_with(',') {
                    lines[previous] = format!("{code},{}", &line[code.len()..]);
                }
            }
            lines.insert(close, format!("{indent}{quoted},"));
        }
        None => {
            lines.insert(first_table, format!("egress_hosts = [{quoted}]"));
            if first_table < lines.len() - 1 {
                lines.insert(first_table + 1, String::new());
            }
        }
    }
    let mut result = lines.join("\n");
    if content.ends_with('\n') || content.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

/// A TOML key as written in a table header: bare when it can be.
fn toml_key(key: &str) -> Result<String> {
    if !key.is_empty()
        && key
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-')
    {
        Ok(key.to_owned())
    } else {
        Ok(serde_json::to_string(key)?)
    }
}

fn credential_rule_snippet(
    table: &str,
    binding: &str,
    host: &str,
    method: &str,
    path: &str,
) -> Result<String> {
    Ok(format!(
        "[[{table}.{}.rules]]\neffect = \"allow\"\nhosts = [{}]\nmethods = [{}]\npaths = [{}]\n",
        toml_key(binding)?,
        serde_json::to_string(host)?,
        serde_json::to_string(method)?,
        serde_json::to_string(path)?,
    ))
}

fn append_credential_rule(
    content: &str,
    table: &str,
    binding: &str,
    host: &str,
    method: &str,
    path: &str,
) -> Result<String> {
    let mut result = content.to_owned();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    if !result.is_empty() && !result.ends_with("\n\n") {
        result.push('\n');
    }
    result.push_str(&credential_rule_snippet(
        table, binding, host, method, path,
    )?);
    Ok(result)
}

/// The snippet a user can paste when the change can't be applied in place.
fn manual_snippet(change: &ProfileChange) -> Result<String> {
    Ok(match change {
        ProfileChange::EgressHost { host } => {
            format!("egress_hosts = [..., {}]\n", serde_json::to_string(host)?)
        }
        ProfileChange::CredentialRule {
            table,
            binding,
            host,
            method,
            path,
        } => credential_rule_snippet(table, binding, host, method, path)?,
        ProfileChange::McpTool { server, tool } => format!(
            "[mcp_servers.{}]\nallow_tools = [..., {}]\n",
            toml_key(server)?,
            serde_json::to_string(tool)?
        ),
    })
}

/// Re-parses and re-validates the edited profile and checks that it now
/// allows the denied request, so a bad edit is never written.
fn check_updated_profile(updated: &str, change: &ProfileChange) -> Result<()> {
    let profile =
        toml::from_str::<AgentProfile>(updated).context("the edited profile does not parse")?;
    ensure_profile_is_valid_for_run(&profile)?;
    let allowed = match change {
        ProfileChange::EgressHost { host } => {
            host_listed(profile.egress_hosts.as_deref(), host)
                && !host_listed(profile.deny_hosts.as_deref(), host)
        }
        ProfileChange::CredentialRule {
            binding,
            host,
            method,
            path,
            ..
        } => binding_entry(&profile, binding).is_some_and(|(_, secret)| {
            credential_decision(secret, host, method, path)
                == SecretAuthorizationDecision::AllowedRule
        }),
        ProfileChange::McpTool { server, tool } => {
            profile.mcp_servers.get(server).is_some_and(|server| {
                server.allow_tools.contains(tool) && !server.deny_tools.contains(tool)
            })
        }
    };
    if !allowed {
        bail!("the edited profile still would not allow the request");
    }
    Ok(())
}

fn find_event(command: &AgentAllowCommand) -> Result<ProxyAuditLogEvent> {
    if let Some(id) = &command.event_id {
        let filter = ProxyAuditLogFilter {
            id: Some(id.clone()),
            ..Default::default()
        };
        let event = read_local_proxy_audit_logs(1, None, &filter)?
            .pop()
            .with_context(|| format!("No local audit event has the ID {id}."))?;
        if event.response_status != Some(403) {
            bail!("Audit event {id} is `{}`, not a denial.", event.action);
        }
        return Ok(event);
    }
    read_local_proxy_audit_logs(AUDIT_EVENT_LIMIT, None, &ProxyAuditLogFilter::default())?
        .into_iter()
        .rev()
        .find(|event| event.response_status == Some(403))
        .context("No denied request in the local audit logs.")
}

/// The profile file the denied request's session ran with.
fn session_profile_path(event: &ProxyAuditLogEvent) -> Result<(PathBuf, Option<String>)> {
    let filter = ProxyAuditLogFilter {
        session: Some(event.session_id.clone()),
        action: Some("session_started".to_owned()),
        ..Default::default()
    };
    let started = read_local_proxy_audit_logs(1, None, &filter)?
        .pop()
        .context(
            "The run's session_started event is missing; pass --policy-file to choose the profile.",
        )?;
    let source = started.profile_source.context(
        "The run did not record its profile file; pass --policy-file to choose the profile.",
    )?;
    Ok((PathBuf::from(source), started.profile_file_sha256))
}

fn file_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}

#[derive(Serialize)]
struct AllowReport {
    event_id: String,
    profile_file: String,
    change: ProfileChange,
    applied: bool,
}

pub fn handle_agent_allow_command(
    command: AgentAllowCommand,
    silent: bool,
    json: bool,
) -> Result<()> {
    let event = find_event(&command)?;
    let (path, recorded_sha256) = match &command.policy_file {
        Some(path) => (path.clone(), None),
        None => session_profile_path(&event)?,
    };
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Could not read agent profile '{}'.", path.display()))?;
    if recorded_sha256.is_some_and(|recorded| file_sha256(&path).ok() != Some(recorded)) {
        eprintln!(
            "Warning: {} changed after the run; checking the denial against its current contents.",
            path.display()
        );
    }
    let profile = toml::from_str::<AgentProfile>(&content)
        .with_context(|| format!("Could not parse agent profile '{}'.", path.display()))?;
    let change = denial_change(&event, &profile)
        .map_err(|why| anyhow::anyhow!("Can't allow {} automatically: {why}.", event.event_id))?;
    let updated = apply_change(&content, &change)
        .and_then(|updated| check_updated_profile(&updated, &change).map(|()| updated));
    let updated = match updated {
        Ok(updated) => updated,
        Err(error) => bail!(
            "Could not change {} safely ({error:#}). Add this by hand:\n\n{}",
            path.display(),
            manual_snippet(&change)?
        ),
    };

    let report = |applied| AllowReport {
        event_id: event.event_id.clone(),
        profile_file: path.display().to_string(),
        change: change.clone(),
        applied,
    };
    if json && !command.yes {
        println!("{}", get_formatted_json_string(&report(false), true)?);
        return Ok(());
    }
    if !json {
        println!("Profile: {}", path.display());
        println!("Change: {}", change.describe());
        println!();
        for line in added_lines(&content, &updated) {
            println!("+ {line}");
        }
        println!();
    }
    if !command.yes {
        if silent {
            bail!("Pass --yes to change the profile without a prompt.");
        }
        match crate::utils::interaction::confirm_opt("Save this change to the profile?") {
            Some(true) => {}
            Some(false) | None => bail!("The profile was not changed."),
        }
    }
    fs::write(&path, &updated)
        .with_context(|| format!("Could not write agent profile '{}'.", path.display()))?;
    if json {
        println!("{}", get_formatted_json_string(&report(true), true)?);
    } else {
        println!(
            "Updated {}. The change applies from the next run.",
            path.display()
        );
    }
    Ok(())
}

/// Lines in `updated` that `original` doesn't have, in order: the edits
/// only ever insert or extend lines, so this is the whole diff.
fn added_lines<'a>(original: &str, updated: &'a str) -> Vec<&'a str> {
    let mut remaining = original.lines().collect::<Vec<_>>();
    remaining.reverse();
    let mut added = Vec::new();
    for line in updated.lines() {
        if remaining.last() == Some(&line) {
            remaining.pop();
        } else {
            added.push(line);
        }
    }
    added
}

/// One line of the end-of-run summary: denials of the same request shape.
#[derive(Debug, PartialEq, Eq)]
struct DenialGroup {
    label: String,
    count: usize,
    /// Newest event of the group, when `agent allow` may fix it.
    allow_event_id: Option<String>,
}

fn denial_label(event: &ProxyAuditLogEvent) -> String {
    let host = event.destination_host.as_deref().unwrap_or("-");
    match event.action.as_str() {
        "host_denied" => match &event.binding_name {
            Some(binding) => format!("{binding} {host}"),
            None => format!("host {host}"),
        },
        "credential_rule_denied" => format!(
            "{} {} {host}{}",
            event.binding_name.as_deref().unwrap_or("credential"),
            event.method.as_deref().unwrap_or("-"),
            event.path.as_deref().unwrap_or_default(),
        ),
        "mcp_tool_denied" => format!(
            "MCP {} on {host}",
            event.mcp_tool.as_deref().unwrap_or("request")
        ),
        "filesystem_denied" => format!(
            "{} {}",
            event.operation.as_deref().unwrap_or("access"),
            event.path.as_deref().unwrap_or("-")
        ),
        action => format!("{action} {host}"),
    }
}

fn group_denials(events: &[ProxyAuditLogEvent]) -> Vec<DenialGroup> {
    // Keyed by label; the value keeps first-seen order for stable output.
    let mut groups = BTreeMap::<String, (usize, usize, Option<String>)>::new();
    for (order, event) in events
        .iter()
        .filter(|event| event.response_status == Some(403))
        .enumerate()
    {
        let group = groups
            .entry(denial_label(event))
            .or_insert((order, 0, None));
        group.1 += 1;
        if may_be_fixable(event) {
            group.2 = Some(event.event_id.clone());
        }
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(_, left), (_, right)| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    groups
        .into_iter()
        .map(|(label, (_, count, allow_event_id))| DenialGroup {
            label,
            count,
            allow_event_id,
        })
        .collect()
}

fn format_denial_summary(groups: &[DenialGroup], session_id: &str) -> Option<String> {
    let total = groups.iter().map(|group| group.count).sum::<usize>();
    if total == 0 {
        return None;
    }
    let requests = if total == 1 { "request" } else { "requests" };
    let mut output = format!("Agent Proxy blocked {total} {requests} in this run:\n");
    let width = groups
        .iter()
        .take(SUMMARY_GROUP_LIMIT)
        .map(|group| group.label.len())
        .max()
        .unwrap_or_default();
    for group in groups.iter().take(SUMMARY_GROUP_LIMIT) {
        let count = format!("{}×", group.count);
        match &group.allow_event_id {
            Some(id) => output.push_str(&format!(
                "  {count:>4} {:<width$}  stashbase agent allow {id}\n",
                group.label
            )),
            None => output.push_str(&format!("  {count:>4} {}\n", group.label)),
        }
    }
    if groups.len() > SUMMARY_GROUP_LIMIT {
        output.push_str(&format!(
            "  ... and {} more\n",
            groups.len() - SUMMARY_GROUP_LIMIT
        ));
    }
    output.push_str(&format!(
        "Details: stashbase agent logs --session {session_id}\n"
    ));
    Some(output)
}

/// Prints what the proxy denied during the session, with the `agent allow`
/// command for each fixable denial. Never fails the run.
pub fn print_run_denial_summary(session_id: &str) {
    let filter = ProxyAuditLogFilter {
        session: Some(session_id.to_owned()),
        ..Default::default()
    };
    let Ok(events) = read_local_proxy_audit_logs(AUDIT_EVENT_LIMIT, None, &filter) else {
        return;
    };
    if let Some(summary) = format_denial_summary(&group_denials(&events), session_id) {
        eprint!("{summary}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(action: &str) -> ProxyAuditLogEvent {
        serde_json::from_value(serde_json::json!({
            "timestamp": "2026-10-29T10:00:00Z",
            "session_id": "ses_1",
            "profile": "dev",
            "policy_fingerprint": "fp",
            "event_id": "evt_1",
            "action": action,
            "destination_host": "api.github.com",
            "method": "POST",
            "binding_name": null,
            "response_status": 403,
            "duration_ms": 1,
        }))
        .unwrap()
    }

    fn credential_event(path: &str) -> ProxyAuditLogEvent {
        ProxyAuditLogEvent {
            binding_name: Some("GITHUB_TOKEN".to_owned()),
            path: Some(path.to_owned()),
            ..event("credential_rule_denied")
        }
    }

    const PROFILE: &str = r#"# Agent profile.
egress_hosts = ["registry.npmjs.org"] # package installs
deny_hosts = ["evil.example.com"]

[secrets]
project = "app"
environment = "dev"

[secrets.GITHUB_TOKEN]

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/o/r/*"]

[[secrets.GITHUB_TOKEN.rules]]
effect = "deny"
hosts = ["api.github.com"]
methods = ["DELETE"]
paths = ["*"]

[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
allow_tools = ["list_issues"]
"#;

    fn profile() -> AgentProfile {
        toml::from_str(PROFILE).unwrap()
    }

    fn apply(event: &ProxyAuditLogEvent) -> String {
        let change = denial_change(event, &profile()).unwrap();
        let updated = apply_change(PROFILE, &change).unwrap();
        check_updated_profile(&updated, &change).unwrap();
        updated
    }

    #[test]
    fn a_denied_host_is_added_to_a_single_line_egress_list_keeping_its_comment() {
        let updated = apply(&ProxyAuditLogEvent {
            destination_host: Some("pypi.org".to_owned()),
            ..event("host_denied")
        });
        assert!(updated.contains(
            "egress_hosts = [\"registry.npmjs.org\", \"pypi.org\"] # package installs\n"
        ));
    }

    #[test]
    fn egress_hosts_edits_cover_empty_multi_line_and_missing_lists() {
        assert_eq!(
            add_egress_host("egress_hosts = []\n", "pypi.org").unwrap(),
            "egress_hosts = [\"pypi.org\"]\n"
        );
        assert_eq!(
            add_egress_host(
                "egress_hosts = [\n  \"a.example.com\" # first\n]\n",
                "pypi.org"
            )
            .unwrap(),
            "egress_hosts = [\n  \"a.example.com\", # first\n  \"pypi.org\",\n]\n"
        );
        assert_eq!(
            add_egress_host("# profile\n[secrets.X]\n", "pypi.org").unwrap(),
            "# profile\negress_hosts = [\"pypi.org\"]\n\n[secrets.X]\n"
        );
        // A key inside a table is not the top-level list.
        let nested = "[sandbox]\negress_hosts = []\n";
        let updated = add_egress_host(nested, "pypi.org").unwrap();
        assert!(updated.starts_with("egress_hosts = [\"pypi.org\"]\n"));
    }

    #[test]
    fn a_credential_denial_appends_the_narrowest_allow_rule() {
        let updated = apply(&credential_event("/repos/o/r/issues"));
        assert!(updated.starts_with(PROFILE));
        assert!(updated.ends_with(
            "\n[[secrets.GITHUB_TOKEN.rules]]\neffect = \"allow\"\nhosts = [\"api.github.com\"]\n\
             methods = [\"POST\"]\npaths = [\"/repos/o/r/issues\"]\n"
        ));
    }

    #[test]
    fn an_mcp_tool_denial_extends_allow_tools() {
        let updated = apply(&ProxyAuditLogEvent {
            destination_host: Some("mcp.linear.app".to_owned()),
            path: Some("/mcp".to_owned()),
            mcp_tool: Some("save_issue".to_owned()),
            reason: Some("tool_not_allowed".to_owned()),
            ..event("mcp_tool_denied")
        });
        assert!(updated.contains("allow_tools = [\"list_issues\", \"save_issue\"]"));
    }

    #[test]
    fn denials_the_profile_decided_on_purpose_are_not_changed() {
        let refused = |event: ProxyAuditLogEvent| denial_change(&event, &profile()).unwrap_err();

        assert!(refused(ProxyAuditLogEvent {
            method: Some("DELETE".to_owned()),
            ..credential_event("/repos/o/r")
        })
        .contains("deny rule"));
        assert!(refused(ProxyAuditLogEvent {
            reason: Some("allow_condition_failed:body.state".to_owned()),
            ..credential_event("/repos/o/r/issues")
        })
        .contains("conditions"));
        assert!(refused(ProxyAuditLogEvent {
            destination_host: Some("evil.example.com".to_owned()),
            ..event("host_denied")
        })
        .contains("deny_hosts"));
        assert!(refused(ProxyAuditLogEvent {
            destination_host: Some("registry.npmjs.org".to_owned()),
            ..event("host_denied")
        })
        .contains("already"));
        assert!(refused(ProxyAuditLogEvent {
            destination_host: Some("mcp.linear.app".to_owned()),
            path: Some("/mcp".to_owned()),
            mcp_tool: Some("save_issue".to_owned()),
            reason: Some("tool_denied".to_owned()),
            ..event("mcp_tool_denied")
        })
        .contains("edit them by hand"));
        assert!(refused(event("filesystem_denied")).contains("[filesystem]"));
    }

    #[test]
    fn an_edit_that_still_would_not_allow_the_request_is_rejected() {
        let change = ProfileChange::CredentialRule {
            table: "secrets",
            binding: "GITHUB_TOKEN".to_owned(),
            host: "api.github.com".to_owned(),
            method: "DELETE".to_owned(),
            path: "/repos/o/r".to_owned(),
        };
        let updated = apply_change(PROFILE, &change).unwrap();
        assert!(check_updated_profile(&updated, &change).is_err());
    }

    #[test]
    fn binding_names_that_are_not_bare_keys_are_quoted() {
        assert_eq!(toml_key("GITHUB_TOKEN").unwrap(), "GITHUB_TOKEN");
        assert_eq!(toml_key("my.token").unwrap(), "\"my.token\"");
    }

    #[test]
    fn added_lines_show_only_the_insertions() {
        assert_eq!(added_lines("a\nb\n", "a\nx\nb\ny\n"), vec!["x", "y"]);
    }

    #[test]
    fn the_run_summary_groups_denials_and_offers_allow_only_where_it_can_help() {
        let host = |id: &str, host: &str| ProxyAuditLogEvent {
            event_id: id.to_owned(),
            destination_host: Some(host.to_owned()),
            ..event("host_denied")
        };
        let events = vec![
            host("evt_a", "pypi.org"),
            ProxyAuditLogEvent {
                response_status: Some(200),
                ..host("evt_ok", "pypi.org")
            },
            host("evt_b", "pypi.org"),
            ProxyAuditLogEvent {
                event_id: "evt_c".to_owned(),
                reason: Some("deny_condition_matched:body.state".to_owned()),
                ..credential_event("/repos/o/r/issues")
            },
        ];
        let groups = group_denials(&events);
        assert_eq!(
            groups,
            vec![
                DenialGroup {
                    label: "host pypi.org".to_owned(),
                    count: 2,
                    allow_event_id: Some("evt_b".to_owned()),
                },
                DenialGroup {
                    label: "GITHUB_TOKEN POST api.github.com/repos/o/r/issues".to_owned(),
                    count: 1,
                    allow_event_id: None,
                },
            ]
        );
        let summary = format_denial_summary(&groups, "ses_1").unwrap();
        assert!(summary.starts_with("Agent Proxy blocked 3 requests in this run:\n"));
        assert!(summary.contains("stashbase agent allow evt_b\n"));
        assert!(summary.ends_with("Details: stashbase agent logs --session ses_1\n"));
        assert_eq!(format_denial_summary(&[], "ses_1"), None);
    }

    #[test]
    fn the_run_summary_caps_its_groups() {
        let groups = (0..SUMMARY_GROUP_LIMIT + 2)
            .map(|index| DenialGroup {
                label: format!("host h{index}"),
                count: 1,
                allow_event_id: None,
            })
            .collect::<Vec<_>>();
        let summary = format_denial_summary(&groups, "ses_1").unwrap();
        assert!(summary.contains("  ... and 2 more\n"));
        assert!(!summary.contains("h11"));
    }
}
