//! Turns hosts the Agent Proxy blocked into `egress_hosts` entries, from one
//! audit event or a whole session's blocks. Other blocks are listed with why
//! they need a hand edit.

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
        agent::policy::configured_host_matches,
        agent::validate::ensure_profile_is_valid,
        run::proxy::{read_local_proxy_audit_logs, ProxyAuditLogEvent, ProxyAuditLogFilter},
    },
    models::agent::AgentProfile,
    utils::output::get_formatted_json_string,
};

/// Enough to cover any session the local audit retention keeps.
const AUDIT_EVENT_LIMIT: usize = 100_000;

/// The host to add to `egress_hosts` so the request in `event` is allowed,
/// or why `agent allow` leaves it to a hand edit.
fn egress_fix(
    event: &ProxyAuditLogEvent,
    profile: &AgentProfile,
) -> std::result::Result<String, String> {
    let host = event.destination_host.clone().unwrap_or_default();
    match event.action.as_str() {
        "host_denied" if event.binding_name.is_some() => Err(format!(
            "{}'s `hosts` list doesn't include {host}; edit it by hand",
            event.binding_name.as_deref().unwrap_or_default()
        )),
        "host_denied" if host.is_empty() => Err("the block doesn't name a host".to_owned()),
        "host_denied" => {
            if host_listed(profile.deny_hosts.as_deref(), &host) {
                return Err("in deny_hosts; remove it there if the agent needs it".to_owned());
            }
            if host_listed(profile.egress_hosts.as_deref(), &host) {
                return Err("already in egress_hosts".to_owned());
            }
            Ok(host)
        }
        "credential_rule_denied" => Err(format!(
            "{}'s rules decided this; see `stashbase agent explain --profile {} --host {host} \
             --method {} --path {}`",
            event.binding_name.as_deref().unwrap_or("the credential"),
            event.profile,
            event.method.as_deref().unwrap_or("GET"),
            event.path.as_deref().unwrap_or("/"),
        )),
        "mcp_tool_denied" => Err(
            "the MCP server's allow_tools or tool_rules decided this; edit them by hand".to_owned(),
        ),
        "filesystem_denied" => {
            Err("[filesystem] deny_read/deny_write decided this; edit it by hand".to_owned())
        }
        action => Err(format!("`{action}` blocks aren't decided by egress_hosts")),
    }
}

fn host_listed(hosts: Option<&[String]>, host: &str) -> bool {
    hosts.is_some_and(|hosts| {
        hosts
            .iter()
            .any(|listed| listed == "*" || configured_host_matches(listed, host))
    })
}

/// The part of a TOML line before a comment. Hosts and the keys we look for
/// never contain `#`, so a plain split is enough here.
fn code_part(line: &str) -> &str {
    line.split('#').next().unwrap_or(line)
}

/// The profile text with `host` added to `egress_hosts`. The edit is
/// line-based so comments and layout survive; callers re-check the result.
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

/// Re-parses and re-validates the edited profile and checks that it now
/// reaches `host`, so a bad edit is never written.
fn check_updated_profile(updated: &str, host: &str) -> Result<()> {
    let profile =
        toml::from_str::<AgentProfile>(updated).context("the edited profile does not parse")?;
    ensure_profile_is_valid(&profile)?;
    if !host_listed(profile.egress_hosts.as_deref(), host)
        || host_listed(profile.deny_hosts.as_deref(), host)
    {
        bail!("the edited profile still would not reach {host}");
    }
    Ok(())
}

/// The denial `id` names.
fn find_event(id: &str) -> Result<ProxyAuditLogEvent> {
    let filter = ProxyAuditLogFilter {
        id: Some(id.to_owned()),
        ..Default::default()
    };
    let event = read_local_proxy_audit_logs(1, None, &filter)?
        .pop()
        .with_context(|| format!("No local audit event has the ID {id}."))?;
    if event.response_status != Some(403) {
        bail!("Audit event {id} is `{}`, not a denial.", event.action);
    }
    Ok(event)
}

/// The most recently started session in the local audit logs.
fn latest_session_id() -> Result<String> {
    let filter = ProxyAuditLogFilter {
        action: Some("session_started".to_owned()),
        ..Default::default()
    };
    Ok(read_local_proxy_audit_logs(1, None, &filter)?
        .pop()
        .context("No agent run in the local audit logs.")?
        .session_id)
}

fn session_events(session_id: &str) -> Result<Vec<ProxyAuditLogEvent>> {
    let filter = ProxyAuditLogFilter {
        session: Some(session_id.to_owned()),
        ..Default::default()
    };
    read_local_proxy_audit_logs(AUDIT_EVENT_LIMIT, None, &filter)
}

/// The profile file the session ran with.
fn session_profile_path(session_id: &str) -> Result<(PathBuf, Option<String>)> {
    let filter = ProxyAuditLogFilter {
        session: Some(session_id.to_owned()),
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

/// Denials of the same request shape within a session.
#[derive(Debug)]
struct DenialGroup {
    label: String,
    count: usize,
    /// The newest denial of the group.
    event: ProxyAuditLogEvent,
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

/// The session's denials grouped by request shape, most frequent first.
fn group_denials(events: &[ProxyAuditLogEvent]) -> Vec<DenialGroup> {
    // Keyed by label; the order keeps first-seen order among equal counts.
    let mut groups = BTreeMap::<String, (usize, DenialGroup)>::new();
    for (order, event) in events
        .iter()
        .filter(|event| event.response_status == Some(403))
        .enumerate()
    {
        let label = denial_label(event);
        let entry = groups.entry(label.clone()).or_insert_with(|| {
            (
                order,
                DenialGroup {
                    label,
                    count: 0,
                    event: event.clone(),
                },
            )
        });
        entry.1.count += 1;
        entry.1.event = event.clone();
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by(|(left_order, left), (right_order, right)| {
        right
            .count
            .cmp(&left.count)
            .then(left_order.cmp(right_order))
    });
    groups.into_iter().map(|(_, group)| group).collect()
}

/// Adds every host in order, checking each, so a bad edit is never written.
fn add_egress_hosts(content: &str, hosts: &[&str], path: &Path) -> Result<String> {
    let mut updated = content.to_owned();
    for host in hosts {
        let added = add_egress_host(&updated, host)
            .and_then(|next| check_updated_profile(&next, host).map(|()| next));
        match added {
            Ok(next) => updated = next,
            Err(error) => bail!(
                "Could not change {} safely ({error:#}). Add {} to egress_hosts by hand.",
                path.display(),
                serde_json::to_string(host)?
            ),
        }
    }
    Ok(updated)
}

#[derive(Serialize)]
struct AllowReport {
    session_id: String,
    profile_file: String,
    blocked: Vec<AllowReportEntry>,
    applied: bool,
}

#[derive(Serialize)]
struct AllowReportEntry {
    event_id: String,
    label: String,
    count: usize,
    /// The host `agent allow` would add to `egress_hosts`.
    #[serde(skip_serializing_if = "Option::is_none")]
    egress_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_fixable: Option<String>,
    selected: bool,
}

pub fn handle_agent_allow_command(
    command: AgentAllowCommand,
    silent: bool,
    json: bool,
) -> Result<()> {
    // One event, or every denial of a session.
    let (session_id, groups, single) = match &command.event_id {
        Some(id) => {
            let event = find_event(id)?;
            let group = DenialGroup {
                label: denial_label(&event),
                count: 1,
                event,
            };
            (group.event.session_id.clone(), vec![group], true)
        }
        None => {
            let session_id = match &command.session {
                Some(session_id) => session_id.clone(),
                None => latest_session_id()?,
            };
            let events = session_events(&session_id)?;
            let groups = group_denials(&events);
            if groups.is_empty() {
                if !json {
                    println!("No requests were blocked in session {session_id}.");
                } else {
                    println!("{}", get_formatted_json_string(&Vec::<()>::new(), true)?);
                }
                return Ok(());
            }
            (session_id, groups, false)
        }
    };

    let (path, recorded_sha256) = match &command.policy_file {
        Some(path) => (path.clone(), None),
        None => session_profile_path(&session_id)?,
    };
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Could not read agent profile '{}'.", path.display()))?;
    if recorded_sha256.is_some_and(|recorded| file_sha256(&path).ok() != Some(recorded)) {
        eprintln!(
            "Warning: {} changed after the run; checking the blocks against its current contents.",
            path.display()
        );
    }
    let profile = toml::from_str::<AgentProfile>(&content)
        .with_context(|| format!("Could not parse agent profile '{}'.", path.display()))?;
    let decided = groups
        .iter()
        .map(|group| (group, egress_fix(&group.event, &profile)))
        .collect::<Vec<_>>();
    if single {
        if let Err(why) = &decided[0].1 {
            bail!(
                "Can't allow {} automatically: {why}.",
                decided[0].0.event.event_id
            );
        }
    }

    let fixable = decided
        .iter()
        .filter_map(|(group, fix)| fix.as_ref().ok().map(|host| (*group, host.as_str())))
        .collect::<Vec<_>>();
    let interactive = !silent
        && !json
        && std::io::IsTerminal::is_terminal(&std::io::stdin())
        && std::io::IsTerminal::is_terminal(&std::io::stderr());
    if !json && !single {
        // The picker lists the blocked hosts itself; otherwise list them
        // here with the IDs `agent allow <event-id>` takes.
        println!(
            "Blocked in session {session_id} (profile {}):",
            path.display()
        );
        let width = fixable
            .iter()
            .map(|(group, _)| group.label.len())
            .max()
            .unwrap_or_default();
        for (group, fix) in &decided {
            let count = format!("{}×", group.count);
            match fix {
                Ok(_) if interactive => {}
                Ok(_) => println!(
                    "  {count:>4} {:<width$}  stashbase agent allow {}",
                    group.label, group.event.event_id
                ),
                Err(why) => println!("  {count:>4} {} ({why})", group.label),
            }
        }
        println!();
    }
    let selected = if single || fixable.is_empty() {
        (0..fixable.len()).collect::<Vec<_>>()
    } else if interactive {
        let labels = fixable
            .iter()
            .map(|(group, _)| match group.count {
                1 => group.label.clone(),
                count => format!("{} ({count}×)", group.label),
            })
            .collect::<Vec<_>>();
        dialoguer::MultiSelect::with_theme(
            &crate::handlers::agent::mcp::McpMultiSelectTheme::default(),
        )
        .with_prompt("Select hosts to allow")
        .items(&labels)
        .interact_opt()?
        .unwrap_or_default()
    } else {
        // Without a terminal nothing is chosen for the user.
        Vec::new()
    };
    let selected_ids = selected
        .iter()
        .map(|index| fixable[*index].0.event.event_id.as_str())
        .collect::<Vec<_>>();
    let report = |applied: bool| AllowReport {
        session_id: session_id.clone(),
        profile_file: path.display().to_string(),
        blocked: decided
            .iter()
            .map(|(group, fix)| AllowReportEntry {
                event_id: group.event.event_id.clone(),
                label: group.label.clone(),
                count: group.count,
                egress_host: fix.as_ref().ok().cloned(),
                not_fixable: fix.as_ref().err().cloned(),
                selected: selected_ids.contains(&group.event.event_id.as_str()),
            })
            .collect(),
        applied,
    };
    if fixable.is_empty() {
        if json {
            println!("{}", get_formatted_json_string(&report(false), true)?);
        } else {
            println!("No blocked hosts to add to egress_hosts.");
        }
        return Ok(());
    }
    if selected.is_empty() {
        if json {
            println!("{}", get_formatted_json_string(&report(false), true)?);
        } else if interactive {
            println!("Nothing selected; the profile was not changed.");
        }
        return Ok(());
    }

    let hosts = selected
        .iter()
        .map(|index| fixable[*index].1)
        .collect::<Vec<_>>();
    let updated = add_egress_hosts(&content, &hosts, &path)?;
    if json && !command.yes {
        println!("{}", get_formatted_json_string(&report(false), true)?);
        return Ok(());
    }
    if !json {
        println!("Profile: {}", path.display());
        println!("Add to egress_hosts: {}", hosts.join(", "));
        println!();
        for line in added_lines(&content, &updated) {
            println!("+ {line}");
        }
        println!();
    }
    if !command.yes {
        if !interactive {
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

/// Lines in `updated` that a line diff against `original` marks as added,
/// in order. Profiles are small, so a plain LCS table is fine.
fn added_lines<'a>(original: &str, updated: &'a str) -> Vec<&'a str> {
    let old = original.lines().collect::<Vec<_>>();
    let new = updated.lines().collect::<Vec<_>>();
    // common[i][j]: length of the longest common subsequence of old[i..], new[j..].
    let mut common = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            common[i][j] = if old[i] == new[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut added = Vec::new();
    while j < new.len() {
        if i < old.len() && old[i] == new[j] {
            i += 1;
            j += 1;
        } else if i < old.len() && common[i + 1][j] >= common[i][j + 1] {
            i += 1;
        } else {
            added.push(new[j]);
            j += 1;
        }
    }
    added
}

fn run_denial_notice(events: &[ProxyAuditLogEvent], session_id: &str) -> Option<String> {
    let blocked = events
        .iter()
        .filter(|event| event.response_status == Some(403))
        .count();
    let requests = if blocked == 1 { "request" } else { "requests" };
    (blocked > 0).then(|| {
        format!(
            "Agent Proxy blocked {blocked} {requests} in this run. Review: stashbase agent allow \
             --session {session_id}\n"
        )
    })
}

/// One line after a run that had blocks; `agent allow` shows the details.
/// Never fails the run.
pub fn print_run_denial_notice(session_id: &str) {
    if let Ok(events) = session_events(session_id) {
        if let Some(notice) = run_denial_notice(&events, session_id) {
            eprint!("{notice}");
        }
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

    fn host_event(id: &str, host: &str) -> ProxyAuditLogEvent {
        ProxyAuditLogEvent {
            event_id: id.to_owned(),
            destination_host: Some(host.to_owned()),
            ..event("host_denied")
        }
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
"#;

    fn profile() -> AgentProfile {
        toml::from_str(PROFILE).unwrap()
    }

    #[test]
    fn a_blocked_host_is_added_to_a_single_line_egress_list_keeping_its_comment() {
        let host = egress_fix(&host_event("evt_1", "pypi.org"), &profile()).unwrap();
        let updated = add_egress_hosts(PROFILE, &[&host], Path::new("p.toml")).unwrap();
        assert!(updated.contains(
            "egress_hosts = [\"registry.npmjs.org\", \"pypi.org\"] # package installs\n"
        ));
        assert_eq!(added_lines(PROFILE, &updated).len(), 1);
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
        let updated = add_egress_host("[sandbox]\negress_hosts = []\n", "pypi.org").unwrap();
        assert!(updated.starts_with("egress_hosts = [\"pypi.org\"]\n"));
    }

    #[test]
    fn several_hosts_are_added_in_order() {
        let updated = add_egress_hosts(
            PROFILE,
            &["pypi.org", "files.pythonhosted.org"],
            Path::new("p"),
        )
        .unwrap();
        assert!(
            updated.contains("[\"registry.npmjs.org\", \"pypi.org\", \"files.pythonhosted.org\"]")
        );
    }

    #[test]
    fn only_plain_host_blocks_become_egress_entries() {
        let refused = |event: ProxyAuditLogEvent| egress_fix(&event, &profile()).unwrap_err();

        assert!(refused(host_event("evt_1", "evil.example.com")).contains("deny_hosts"));
        assert!(refused(host_event("evt_1", "registry.npmjs.org")).contains("already"));
        assert!(refused(ProxyAuditLogEvent {
            binding_name: Some("OLD_TOKEN".to_owned()),
            ..host_event("evt_1", "api.example.com")
        })
        .contains("OLD_TOKEN's `hosts` list"));
        assert!(refused(credential_event("/user")).contains(
            "stashbase agent explain --profile dev --host api.github.com --method POST --path /user"
        ));
        assert!(refused(event("mcp_tool_denied")).contains("allow_tools"));
        assert!(refused(event("filesystem_denied")).contains("[filesystem]"));
    }

    #[test]
    fn an_edit_that_would_still_not_reach_the_host_is_rejected() {
        let updated = add_egress_host(PROFILE, "evil.example.com").unwrap();
        assert!(check_updated_profile(&updated, "evil.example.com").is_err());
        let error =
            add_egress_hosts(PROFILE, &["evil.example.com"], Path::new("p.toml")).unwrap_err();
        assert!(error.to_string().contains("by hand"));
    }

    #[test]
    fn added_lines_show_only_the_insertions() {
        assert_eq!(added_lines("a\nb\n", "a\nx\nb\ny\n"), vec!["x", "y"]);
        // A line changed in place is one added line, not everything after it.
        assert_eq!(added_lines("a\nb\nc\n", "a\nB\nc\n"), vec!["B"]);
    }

    #[test]
    fn denials_group_by_request_shape_keeping_the_newest_event() {
        let events = vec![
            ProxyAuditLogEvent {
                event_id: "evt_c".to_owned(),
                ..credential_event("/repos/o/r/issues")
            },
            host_event("evt_a", "pypi.org"),
            ProxyAuditLogEvent {
                response_status: Some(200),
                ..host_event("evt_ok", "pypi.org")
            },
            host_event("evt_b", "pypi.org"),
        ];
        let groups = group_denials(&events)
            .into_iter()
            .map(|group| (group.label, group.count, group.event.event_id))
            .collect::<Vec<_>>();
        assert_eq!(
            groups,
            vec![
                ("host pypi.org".to_owned(), 2, "evt_b".to_owned()),
                (
                    "GITHUB_TOKEN POST api.github.com/repos/o/r/issues".to_owned(),
                    1,
                    "evt_c".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_run_with_blocks_ends_with_one_line_pointing_to_agent_allow() {
        assert_eq!(
            run_denial_notice(&[event("host_denied"), event("host_denied")], "ses_1").as_deref(),
            Some(
                "Agent Proxy blocked 2 requests in this run. Review: stashbase agent allow \
                 --session ses_1\n"
            )
        );
        let allowed = ProxyAuditLogEvent {
            response_status: Some(200),
            ..event("forwarded")
        };
        assert_eq!(run_denial_notice(&[allowed], "ses_1"), None);
    }
}
