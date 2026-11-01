use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::{
    human_datetime::{format_utc, format_utc_timestamp},
    interaction,
    spinner::request_spinner,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAgentSession {
    pub session_id: String,
    pub started_at: String,
    process_id: u32,
    process_started_at: String,
    #[serde(default)]
    revoked: bool,
}

/// Where `stashbase agent mcp connect` reaches a run's stdio MCP servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpBrokerEndpoint {
    pub url: String,
    pub token: String,
}

/// A run's MCP broker, recorded for agents that start MCP servers without
/// the run's environment (Codex clears it), keyed by the run's pid.
#[derive(Debug, Serialize, Deserialize)]
struct McpBrokerRecord {
    process_id: u32,
    /// Unix seconds; the run's process must have started by then.
    registered_at: i64,
    #[serde(flatten)]
    endpoint: McpBrokerEndpoint,
}

/// Keeps a run's broker record for as long as the run lasts.
#[derive(Debug)]
pub struct McpBrokerRegistration(PathBuf);

impl McpBrokerRegistration {
    pub fn register(endpoint: McpBrokerEndpoint) -> Result<Self> {
        let directory =
            mcp_broker_directory().context("cannot locate the Stashbase config directory")?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let record = McpBrokerRecord {
            process_id: std::process::id(),
            registered_at: Utc::now().timestamp(),
            endpoint,
        };
        let path = directory.join(format!("{}.json", record.process_id));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(&serde_json::to_vec(&record)?)?;
        file.sync_all()?;
        Ok(Self(path))
    }
}

impl Drop for McpBrokerRegistration {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Resolved without creating anything: the agent's MCP server process may
/// not write here (Codex confines the servers it starts).
fn mcp_broker_directory() -> Option<PathBuf> {
    Some(
        directories::ProjectDirs::from("", "", "stashbase")?
            .config_dir()
            .join("mcp-brokers"),
    )
}

#[derive(Debug)]
pub struct LocalAgentSessionGuard(PathBuf);

impl LocalAgentSessionGuard {
    pub fn start(session_id: String) -> Result<Self> {
        let directory = session_directory()?;
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        #[cfg(unix)]
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;

        let session = LocalAgentSession {
            session_id: session_id.clone(),
            started_at: format_utc(Utc::now()),
            process_id: std::process::id(),
            process_started_at: process_start_time(std::process::id())?,
            revoked: false,
        };
        let path = directory.join(format!("{session_id}.json"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(&serde_json::to_vec(&session)?)?;
        file.sync_all()?;
        Ok(Self(path))
    }

    pub fn path(&self) -> PathBuf {
        self.0.clone()
    }

    /// The session id this guard was started with, recovered from its
    /// backing file's name (`<session_id>.json`) rather than stored
    /// separately, since the two must always agree.
    pub fn session_id(&self) -> &str {
        self.0
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
    }
}

impl Drop for LocalAgentSessionGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn list_local_sessions() -> Result<Vec<LocalAgentSession>> {
    let directory = session_directory()?;
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(Vec::new());
    };
    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|ext| ext != "json")
        {
            continue;
        }
        let session = match fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LocalAgentSession>(&bytes).ok())
        {
            Some(session) => session,
            None => {
                let _ = fs::remove_file(entry.path());
                continue;
            }
        };
        if process_start_time(session.process_id)
            .is_ok_and(|started| started == session.process_started_at)
        {
            if !session.revoked {
                sessions.push(session);
            }
        } else {
            let _ = fs::remove_file(entry.path());
        }
    }
    sessions.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    Ok(sessions)
}

/// The MCP broker of the run this process was started under: the nearest
/// ancestor process that registered one. Reads the process table directly,
/// since the agent sandbox doesn't allow running `ps`. The error says what
/// was looked at, since the agent only logs it.
pub fn mcp_broker_for_ancestors() -> std::result::Result<McpBrokerEndpoint, String> {
    let directory = mcp_broker_directory().ok_or("cannot locate the Stashbase config directory")?;
    let records = fs::read_dir(&directory)
        .map_err(|error| {
            format!(
                "no run has stdio MCP servers ({}: {error})",
                directory.display()
            )
        })?
        .filter_map(|entry| fs::read(entry.ok()?.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<McpBrokerRecord>(&bytes).ok())
        .collect::<Vec<_>>();
    let runs = records
        .iter()
        .map(|record| record.process_id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mut chain = vec![std::process::id().to_string()];
    let mut pid = std::process::id();
    // Deep enough for agent → shell → npx-style wrappers, short enough that
    // a broken process table can't loop.
    for _ in 0..32 {
        let Some((parent, _)) = process_info(pid) else {
            chain.push("?".to_owned());
            break;
        };
        pid = parent;
        chain.push(pid.to_string());
        if pid <= 1 {
            break;
        }
        let Some(record) = records.iter().find(|record| record.process_id == pid) else {
            continue;
        };
        // The run registered after it started. A process that started later
        // only reuses the pid of one that has exited, and must not be handed
        // that run's broker.
        return match process_info(pid) {
            Some((_, started)) if started <= record.registered_at => Ok(record.endpoint.clone()),
            started => Err(format!(
                "process {pid} reuses the pid of an ended run (started {:?}, registered {})",
                started.map(|(_, started)| started),
                record.registered_at
            )),
        };
    }
    Err(format!(
        "no ancestor of this process is a run with stdio MCP servers (ancestors: {}; runs: {})",
        chain.join(" <- "),
        if runs.is_empty() { "none" } else { &runs }
    ))
}

/// A process's parent pid and start time in Unix seconds.
#[cfg(target_os = "macos")]
fn process_info(pid: u32) -> Option<(u32, i64)> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    Some((info.pbi_ppid, info.pbi_start_tvsec as i64))
}

/// A process's parent pid and start time in Unix seconds.
#[cfg(target_os = "linux")]
fn process_info(pid: u32) -> Option<(u32, i64)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is in parentheses and may contain spaces; the fields
    // after it are space-separated: state, ppid, ..., starttime (22nd).
    let fields = stat
        .get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let parent = fields.get(1)?.parse().ok()?;
    let ticks: i64 = fields.get(19)?.parse().ok()?;
    let boot = fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse::<i64>()
        .ok()?;
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (per_second > 0).then(|| (parent, boot + ticks / per_second))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_info(_pid: u32) -> Option<(u32, i64)> {
    None
}

pub fn revoke_local_session(session_id: &str) -> Result<bool> {
    let path = session_directory()?.join(format!("{session_id}.json"));
    let Ok(bytes) = fs::read(&path) else {
        return Ok(false);
    };
    let mut session: LocalAgentSession = serde_json::from_slice(&bytes)?;
    if !process_start_time(session.process_id)
        .is_ok_and(|started| started == session.process_started_at)
    {
        let _ = fs::remove_file(&path);
        return Ok(false);
    }
    if session.revoked {
        return Ok(false);
    }
    session.revoked = true;
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(&serde_json::to_vec(&session)?)?;
    file.sync_all()?;
    Ok(true)
}

pub fn is_local_session_revoked(path: &std::path::Path) -> bool {
    match fs::read(path).and_then(|bytes| {
        serde_json::from_slice::<LocalAgentSession>(&bytes).map_err(std::io::Error::other)
    }) {
        Ok(session) => session.revoked,
        // A missing, truncated, or otherwise unreadable marker must block
        // proxy traffic until the session state can be trusted again.
        Err(_) => true,
    }
}

pub fn is_valid_agent_session_id(value: &str) -> bool {
    value.strip_prefix("ags_").is_some_and(|suffix| {
        suffix.len() == 22 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

fn session_directory() -> Result<PathBuf> {
    Ok(crate::config::config::get_config_path()?
        .parent()
        .context("Stashbase config path has no parent directory")?
        .join("agent-sessions"))
}

pub(crate) fn process_start_time(pid: u32) -> Result<String> {
    // ponytail: native start-time checks avoid dependencies; use pidfds or platform handles if one-second PID reuse proves insufficient.
    #[cfg(unix)]
    let output = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .output()?;
    #[cfg(windows)]
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {pid} -ErrorAction Stop).StartTime.ToUniversalTime().Ticks"),
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("Local agent process is no longer running.");
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .context("Could not read local agent process start time")
}

#[derive(Debug, Clone, Serialize, Tabled)]
pub struct AgentSessionRow {
    pub origin: String,
    pub id: String,
    pub started_at: String,
}

impl From<LocalAgentSession> for AgentSessionRow {
    fn from(session: LocalAgentSession) -> Self {
        Self {
            origin: "local".to_owned(),
            id: session.session_id,
            started_at: session.started_at,
        }
    }
}

pub fn format_sessions(rows: &Vec<AgentSessionRow>, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            crate::utils::output::get_formatted_json_string(rows, true)?
        );
    } else if rows.is_empty() {
        println!("No active agent sessions.");
    } else {
        println!("{}", crate::utils::tables::build::build_table(rows));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        is_valid_agent_session_id, process_start_time, should_confirm_bulk_revoke,
        validate_bulk_scope, AgentSessionRow,
    };

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn process_info_reads_the_parent_and_a_start_time_no_later_than_now() {
        let (parent, started) = super::process_info(std::process::id()).unwrap();
        assert_eq!(parent, std::os::unix::process::parent_id());
        let now = chrono::Utc::now().timestamp();
        assert!(started > 0 && started <= now, "{started} vs {now}");
        assert!(super::process_info(parent).is_some());
    }

    #[test]
    fn process_identity_is_stable_for_the_current_process() {
        let pid = std::process::id();
        let first = process_start_time(pid).unwrap();
        assert!(!first.is_empty());
        assert_eq!(process_start_time(pid).unwrap(), first);
    }

    #[test]
    fn session_rows_do_not_include_expiration() {
        let row = AgentSessionRow {
            origin: "remote".to_owned(),
            id: "session-id".to_owned(),
            started_at: "2026-07-03T14:10:07Z".to_owned(),
        };
        assert!(serde_json::to_value(row).unwrap().get("command").is_none());
    }

    #[test]
    fn session_ids_require_a_22_character_suffix() {
        assert!(is_valid_agent_session_id("ags_1234567890123456789012"));
        assert!(!is_valid_agent_session_id("ags_short"));
        assert!(!is_valid_agent_session_id("ags_12345678901234567890123"));
    }

    #[test]
    fn bulk_revocation_requires_a_scope() {
        assert!(validate_bulk_scope(true, true, false, false, false).is_ok());
        assert!(validate_bulk_scope(true, false, true, false, false).is_ok());
        assert!(validate_bulk_scope(true, false, false, false, false).is_err());
        assert!(validate_bulk_scope(false, false, false, false, false).is_ok());
    }

    #[test]
    fn silent_bulk_revocation_skips_confirmation() {
        assert!(should_confirm_bulk_revoke(true, false));
        assert!(!should_confirm_bulk_revoke(true, true));
        assert!(!should_confirm_bulk_revoke(false, false));
    }
}

pub async fn handle_sessions(
    command: crate::cmd::agent::AgentSessionsCommand,
    api_key: &str,
    json: bool,
    silent: bool,
) -> Result<()> {
    let show_spinner = !silent && (command.remote || !command.local);
    let spinner = show_spinner.then(request_spinner);
    let remote_sessions = if command.remote || !command.local {
        Some(crate::api::remote_proxy::list_agent_sessions(api_key, json).await)
    } else {
        None
    };
    if let Some(mut spinner) = spinner {
        spinner.stop_and_persist("", "");
    }
    let mut rows: Vec<AgentSessionRow> = if let Some(remote_sessions) = remote_sessions {
        remote_sessions?
            .into_iter()
            .map(|session| {
                Ok(AgentSessionRow {
                    origin: "remote".to_owned(),
                    id: session.id,
                    started_at: format_utc_timestamp(&session.started_at)?,
                })
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    if command.local || !command.remote {
        rows.extend(
            list_local_sessions()?
                .into_iter()
                .map(|session| {
                    let mut row = AgentSessionRow::from(session);
                    row.started_at = format_utc_timestamp(&row.started_at)?;
                    Ok(row)
                })
                .collect::<Result<Vec<_>>>()?,
        );
    }
    rows.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    if !silent && !show_spinner {
        println!();
    }
    format_sessions(&rows, json)
}

pub async fn handle_revoke(
    command: crate::cmd::agent::AgentRevokeCommand,
    api_key: &str,
    json: bool,
    silent: bool,
) -> Result<()> {
    validate_bulk_scope(command.all, command.local, command.remote, json, silent)?;
    if should_confirm_bulk_revoke(command.all, silent) {
        let scope = if command.local { "local" } else { "remote" };
        if interaction::confirm_opt(&format!("Revoke all active {scope} agent sessions?"))
            != Some(true)
        {
            return Ok(());
        }
    }
    let Some(session_id) = command.session_id.as_deref() else {
        return handle_revoke_all(command, api_key, json, silent).await;
    };
    if !is_valid_agent_session_id(session_id) {
        let error = crate::models::validation::InputValidationError::AgentSession(
            crate::models::validation::AgentSessionInputValidationError::InvalidId,
        );
        let formatted = error.format_error_output(json)?;
        if !silent {
            eprintln!();
        }
        return Err(anyhow::anyhow!(formatted));
    }
    let local = if !command.remote {
        revoke_local_session(session_id)?
    } else {
        false
    };
    if local {
        if !silent {
            println!();
        }
        if json {
            println!(
                "{}",
                crate::utils::output::get_formatted_json_string(&serde_json::json!({}), true,)?
            );
        } else {
            println!("Revoked local agent session {}.", session_id);
        }
        return Ok(());
    }
    if command.local {
        anyhow::bail!(
            "No active local agent session found with ID '{}'.",
            session_id
        );
    }
    let mut spinner = (!silent).then(request_spinner);
    let result = crate::api::remote_proxy::revoke_agent_session(api_key, session_id, json).await;
    if let Some(ref mut spinner) = spinner {
        spinner.stop_and_persist("", "");
    }
    result?;
    if json {
        println!(
            "{}",
            crate::utils::output::get_formatted_json_string(&serde_json::json!({}), true,)?
        );
    } else {
        println!("Revoked remote agent session {}.", session_id);
    }
    Ok(())
}

fn validate_bulk_scope(
    all: bool,
    local: bool,
    remote: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    if all && !local && !remote {
        let error = crate::models::validation::InputValidationError::AgentSession(
            crate::models::validation::AgentSessionInputValidationError::BulkScopeRequired,
        );
        let formatted = error.format_error_output(json)?;
        if !silent {
            eprintln!();
        }
        anyhow::bail!(formatted);
    }
    Ok(())
}

fn should_confirm_bulk_revoke(all: bool, silent: bool) -> bool {
    all && !silent
}

async fn handle_revoke_all(
    command: crate::cmd::agent::AgentRevokeCommand,
    api_key: &str,
    json: bool,
    silent: bool,
) -> Result<()> {
    let revoked_session_count = if !command.remote {
        let mut count = 0;
        for session in list_local_sessions()? {
            if revoke_local_session(&session.session_id)? {
                count += 1;
            }
        }
        count
    } else {
        0
    };
    if command.remote {
        let mut spinner = (!silent).then(request_spinner);
        let result = crate::api::remote_proxy::revoke_all_agent_sessions(api_key, json).await;
        if let Some(ref mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        let result = result?;
        if !silent && !json {
            println!(
                "Revoked {} remote agent session(s).",
                result.revoked_session_count
            );
        }
        if json {
            println!(
                "{}",
                crate::utils::output::get_formatted_json_string(&result, true)?
            );
        }
        return Ok(());
    }
    if !silent {
        println!();
    }
    if !silent && !json && revoked_session_count == 0 {
        println!("No active agent sessions to revoke.");
    } else if !silent && !json {
        println!("Revoked {} local agent session(s).", revoked_session_count);
    }
    if json {
        println!(
            "{}",
            crate::utils::output::get_formatted_json_string(
                &serde_json::json!({
                    "origin": "local",
                    "revoked_session_count": revoked_session_count,
                }),
                true,
            )?
        );
    }
    Ok(())
}
