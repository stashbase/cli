use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::Command,
};

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tabled::Tabled;

use crate::utils::spinner::request_spinner;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAgentSession {
    pub session_id: String,
    pub agent: String,
    pub started_at: String,
    process_id: u32,
    process_started_at: String,
}

pub struct LocalAgentSessionGuard(PathBuf);

impl LocalAgentSessionGuard {
    pub fn start(session_id: String, agent: String) -> Result<Self> {
        let directory = session_directory()?;
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        #[cfg(unix)]
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;

        let session = LocalAgentSession {
            session_id: session_id.clone(),
            agent,
            started_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            process_id: std::process::id(),
            process_started_at: process_start_time(std::process::id())?,
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
            sessions.push(session);
        } else {
            let _ = fs::remove_file(entry.path());
        }
    }
    sessions.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    Ok(sessions)
}

pub fn revoke_local_session(session_id: &str) -> Result<bool> {
    let Some(session) = list_local_sessions()?
        .into_iter()
        .find(|session| session.session_id == session_id)
    else {
        return Ok(false);
    };
    if !process_start_time(session.process_id)
        .is_ok_and(|started| started == session.process_started_at)
    {
        let _ = fs::remove_file(session_directory()?.join(format!("{session_id}.json")));
        return Ok(false);
    }
    #[cfg(unix)]
    let children = child_processes(session.process_id)?;
    #[cfg(unix)]
    let result = if children.is_empty() {
        -1
    } else {
        children
            .iter()
            .map(|pid| unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) })
            .find(|result| *result != 0)
            .unwrap_or(0)
    };
    #[cfg(windows)]
    let result = Command::new("taskkill")
        .args(["/PID", &session.process_id.to_string(), "/T", "/F"])
        .output()?
        .status;
    #[cfg(not(any(unix, windows)))]
    let result: i32 = -1;
    #[cfg(windows)]
    if result.success() {
        Ok(true)
    } else if !result.success() {
        anyhow::bail!("Could not stop the local agent session.")
    }
    #[cfg(unix)]
    if result == 0 {
        Ok(true)
    } else {
        Err(std::io::Error::last_os_error()).context("Could not stop the local agent session")
    }
}

#[cfg(unix)]
fn child_processes(parent: u32) -> Result<Vec<u32>> {
    let output = Command::new("pgrep")
        .args(["-P", &parent.to_string()])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(pid) = line.trim().parse() {
            result.push(pid);
            result.extend(child_processes(pid)?);
        }
    }
    Ok(result)
}

fn session_directory() -> Result<PathBuf> {
    Ok(crate::config::config::get_config_path()?
        .parent()
        .context("Stashbase config path has no parent directory")?
        .join("agent-sessions"))
}

fn process_start_time(pid: u32) -> Result<String> {
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
    pub agent: String,
    pub started_at: String,
}

impl From<LocalAgentSession> for AgentSessionRow {
    fn from(session: LocalAgentSession) -> Self {
        Self {
            origin: "local".to_owned(),
            id: session.session_id,
            agent: session.agent,
            started_at: session.started_at,
        }
    }
}

fn format_utc_timestamp(value: &str) -> Result<String> {
    Ok(DateTime::parse_from_rfc3339(value)?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true))
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
    use super::{format_utc_timestamp, process_start_time, AgentSessionRow};

    #[test]
    fn process_identity_is_stable_for_the_current_process() {
        let pid = std::process::id();
        let first = process_start_time(pid).unwrap();
        assert!(!first.is_empty());
        assert_eq!(process_start_time(pid).unwrap(), first);
    }

    #[test]
    fn session_timestamps_are_utc_seconds() {
        assert_eq!(
            format_utc_timestamp("2026-09-13T11:53:59.237435+00:00").unwrap(),
            "2026-09-13T11:53:59Z"
        );
        assert_eq!(
            format_utc_timestamp("2026-07-03T16:10:07+02:00").unwrap(),
            "2026-07-03T14:10:07Z"
        );
    }

    #[test]
    fn session_rows_do_not_include_expiration() {
        let row = AgentSessionRow {
            origin: "remote".to_owned(),
            id: "session-id".to_owned(),
            agent: "codex".to_owned(),
            started_at: "2026-07-03T14:10:07Z".to_owned(),
        };
        assert!(serde_json::to_value(row)
            .unwrap()
            .get("expires_at")
            .is_none());
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
                    agent: "unknown".to_owned(),
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
    let local = if !command.remote {
        revoke_local_session(&command.session_id)?
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
                crate::utils::output::get_formatted_json_string(
                    &serde_json::json!({"session_id": command.session_id, "origin": "local", "revoked": true}),
                    true,
                )?
            );
        } else {
            println!("Revoked local agent session {}.", command.session_id);
        }
        return Ok(());
    }
    if command.local {
        anyhow::bail!(
            "No active local agent session found with ID '{}'.",
            command.session_id
        );
    }
    let mut spinner = (!silent).then(request_spinner);
    let result =
        crate::api::remote_proxy::revoke_agent_session(api_key, &command.session_id, json).await;
    if let Some(ref mut spinner) = spinner {
        spinner.stop_and_persist("", "");
    }
    result?;
    if !silent {
        println!();
    }
    if json {
        println!(
            "{}",
            crate::utils::output::get_formatted_json_string(
                &serde_json::json!({"session_id": command.session_id, "origin": "remote", "revoked": true}),
                true,
            )?
        );
    } else {
        println!("Revoked remote agent session {}.", command.session_id);
    }
    Ok(())
}
