use std::collections::HashMap;
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::ExitStatus;

use anyhow::Result;
use duct::{cmd, Expression};
use thiserror::Error;

// use log::debug;
#[cfg(not(unix))]
use std::io::{BufRead, BufReader};

const RESTRICTED_CHILD_ENV_REMOVALS: &[&str] = &["STASHBASE_API_KEY"];
// These settings can make a child select another proxy or skip the proxy for
// selected hosts. The proxy re-adds its own HTTP(S)_PROXY values afterwards.
const PROXY_CHILD_ENV_REMOVALS: &[&str] = &[
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NPM_CONFIG_NOPROXY",
    "npm_config_noproxy",
    "NPM_CONFIG_PROXY",
    "npm_config_proxy",
    "NPM_CONFIG_HTTPS_PROXY",
    "npm_config_https_proxy",
];

// for now stdout to stderr - working great
#[derive(Debug, Error)]
#[error("command exited with status {status}")]
pub struct CommandFailed {
    pub status: ExitStatus,
}

impl CommandFailed {
    pub fn exit_code(&self) -> i32 {
        self.status.code().unwrap_or(1)
    }
}

pub async fn run_command(
    command: &str,
    args: Vec<String>,
    env_vars: HashMap<String, String>,
    env_removals: Vec<String>,
    sandbox: bool,
    proxy_mode: bool,
    restrict_stashbase_credentials: bool,
) -> Result<ExitStatus> {
    run_command_with_denied_commands(
        command,
        args,
        env_vars,
        env_removals,
        sandbox,
        proxy_mode,
        restrict_stashbase_credentials,
        &[],
        &[],
        &[],
        None,
    )
    .await
}

pub async fn run_command_with_denied_commands(
    command: &str,
    args: Vec<String>,
    mut env_vars: HashMap<String, String>,
    env_removals: Vec<String>,
    sandbox: bool,
    proxy_mode: bool,
    restrict_stashbase_credentials: bool,
    denied_commands: &[String],
    denied_read_paths: &[String],
    denied_write_paths: &[String],
    audit_log: Option<super::proxy::ProxyAuditLog>,
) -> Result<ExitStatus> {
    let current_dir = env::current_dir()?;
    let command_wrappers = CommandWrappers::create(denied_commands)?;
    if let Some(wrappers) = &command_wrappers {
        let inherited_path = env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(wrappers.path.clone().into_os_string())
                .chain(std::env::split_paths(&inherited_path).map(|path| path.into_os_string())),
        )?;
        env_vars.insert("PATH".to_owned(), path.to_string_lossy().into_owned());
        if let Some(event_path) = &wrappers.event_path {
            env_vars.insert(
                "STASHBASE_COMMAND_EVENT_FILE".to_owned(),
                event_path.to_string_lossy().into_owned(),
            );
        }
    }
    let (program, launcher_args) = sandbox_command_with_denied_commands(
        command,
        sandbox,
        &env_vars,
        denied_commands,
        denied_read_paths,
        denied_write_paths,
    )?;
    let cmd: Expression = cmd(program, launcher_args)
        .before_spawn(move |cmd| {
            // Agent profiles may rename a project secret for the child process.
            // Never let an identically named parent environment variable bypass
            // that binding and expose the source name alongside the placeholder.
            for name in &env_removals {
                cmd.env_remove(name);
            }
            if restrict_stashbase_credentials {
                // Do not inherit the developer's Stashbase API key into a
                // restricted agent child. Explicit profile placeholders are
                // added below and remain supported.
                for name in RESTRICTED_CHILD_ENV_REMOVALS {
                    cmd.env_remove(name);
                }
            }
            if proxy_mode {
                // Clear parent and tool-specific proxy overrides before applying
                // the proxy's explicit proxy environment below. `NO_PROXY` is
                // then set to an empty value by Proxy::child_env().
                for name in PROXY_CHILD_ENV_REMOVALS {
                    cmd.env_remove(name);
                }
                // Claude Code gives ANTHROPIC_AUTH_TOKEN higher precedence than
                // ANTHROPIC_API_KEY, so a profile-managed key must replace both.
                if env_vars.contains_key("ANTHROPIC_API_KEY") {
                    cmd.env_remove("ANTHROPIC_AUTH_TOKEN");
                    cmd.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
                }
            }
            for arg in args.iter() {
                cmd.arg(arg);
            }
            for (env, value) in env_vars.iter() {
                cmd.env(env, value);
            }
            Ok(())
        })
        .env("FORCE_COLOR", "true")
        .dir(current_dir);
    // .full_env(env_vars);
    // .env("--color", "always");

    // Unix keeps stdout attached to the terminal while stderr is captured for
    // policy-denial normalization. This avoids breaking interactive agents.
    #[cfg(unix)]
    let status = {
        let terminal_output = unsafe { libc::dup(std::io::stderr().as_raw_fd()) };
        if terminal_output < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let output = cmd
            .stdout_file(terminal_output)
            .stderr_capture()
            .unchecked()
            .run()?;
        emit_child_stderr(
            &output.stderr,
            denied_read_paths,
            denied_write_paths,
            audit_log.as_ref(),
        )?;
        output.status
    };

    #[cfg(not(unix))]
    let status = {
        let mut reader = cmd.stdout_to_stderr().unchecked().reader()?;
        {
            let mut lines = BufReader::new(&mut reader).lines();
            while let Some(line) = lines.next() {
                println!("{}", line?);
            }
        }
        reader
            .try_wait()?
            .expect("reader reached EOF after child exit")
            .status
            .clone()
    };
    if let (Some(wrappers), Some(audit_log)) = (&command_wrappers, &audit_log) {
        if let Some(event_path) = &wrappers.event_path {
            if let Ok(events) = fs::read_to_string(event_path) {
                for command in events
                    .lines()
                    .map(str::trim)
                    .filter(|command| !command.is_empty())
                {
                    audit_log.record_command_denied(command);
                }
            }
        }
    }
    Ok(status)
}

/// A deliberately small first version of command enforcement. The wrappers
/// shadow normal PATH lookups for descendants of the agent and fail closed.
/// Absolute executable paths and shell built-ins are outside this layer.
struct CommandWrappers {
    path: PathBuf,
    event_path: Option<PathBuf>,
}

impl CommandWrappers {
    fn create(commands: &[String]) -> Result<Option<Self>> {
        let commands = commands
            .iter()
            .map(|command| command.trim())
            .filter(|command| {
                !command.is_empty()
                    && command.chars().all(|character| {
                        character.is_ascii_alphanumeric() || "._+-".contains(character)
                    })
            })
            .collect::<Vec<_>>();
        if commands.is_empty() {
            return Ok(None);
        }

        let path =
            env::temp_dir().join(format!("stashbase-command-policy-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        let event_path = path.join("events");
        fs::write(&event_path, "")?;
        let result = (|| {
            for command in commands {
                #[cfg(windows)]
                let wrapper = path.join(format!("{command}.cmd"));
                #[cfg(not(windows))]
                let wrapper = path.join(command);
                #[cfg(windows)]
                let contents = format!(
                    "@>>\"%STASHBASE_COMMAND_EVENT_FILE%\" echo {command}\r\n@echo {{\"error\":{{\"code\":\"command_denied\",\"message\":\"Command denied by agent policy\",\"command\":\"{command}\"}}}} 1>&2\r\n@echo Stashbase agent policy denied command: {command} 1>&2\r\n@exit /b 126\r\n"
                );
                #[cfg(not(windows))]
                let contents = format!(
                    "#!/bin/sh\nprintf '%s\\n' '{command}' >> \"$STASHBASE_COMMAND_EVENT_FILE\"\nprintf '%s\\n' '{{\"error\":{{\"code\":\"command_denied\",\"message\":\"Command denied by agent policy\",\"command\":\"{command}\"}}}}' >&2\nprintf '%s\\n' 'Stashbase agent policy denied command: {command}' >&2\nexit 126\n"
                );
                fs::write(&wrapper, contents)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))?;
                }
            }
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        Ok(Some(Self {
            path,
            event_path: Some(event_path),
        }))
    }
}

impl Drop for CommandWrappers {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
fn sandbox_command(
    command: &str,
    sandbox: bool,
    env_vars: &HashMap<String, String>,
) -> Result<(String, Vec<String>)> {
    sandbox_command_with_denied_commands(command, sandbox, env_vars, &[], &[], &[])
}

fn sandbox_command_with_denied_commands(
    command: &str,
    sandbox: bool,
    env_vars: &HashMap<String, String>,
    denied_commands: &[String],
    denied_read_paths: &[String],
    denied_write_paths: &[String],
) -> Result<(String, Vec<String>)> {
    if !sandbox {
        #[cfg(all(target_os = "linux"))]
        if denied_read_paths.is_empty() && denied_write_paths.is_empty() {
            // Command denials are enforced by the PATH-prepended wrappers in
            // run_command_with_denied_commands. Avoid systemd-run for that
            // layer: CI runners may not support all service properties.
            return Ok((command.to_owned(), Vec::new()));
        }
        #[cfg(all(target_os = "linux"))]
        if !denied_read_paths.is_empty() || !denied_write_paths.is_empty() {
            #[cfg(target_os = "linux")]
            {
                if systemd_filesystem_enforcement_error().is_some() {
                    if let Some(error) = bubblewrap_enforcement_error() {
                        anyhow::bail!(error);
                    }
                    return Ok(bubblewrap_command(
                        command,
                        denied_read_paths,
                        denied_write_paths,
                    ));
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            anyhow::bail!("filesystem restrictions require macOS Seatbelt or Linux filesystem sandbox support");
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
        if !denied_read_paths.is_empty() || !denied_write_paths.is_empty() {
            anyhow::bail!(
                "filesystem restrictions require macOS Seatbelt or Linux systemd user support"
            );
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
        return Ok((command.to_owned(), Vec::new()));
        #[cfg(target_os = "macos")]
        if denied_commands.is_empty()
            && denied_read_paths.is_empty()
            && denied_write_paths.is_empty()
        {
            return Ok((command.to_owned(), Vec::new()));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let network_rules = if sandbox {
            let proxy_port = env_vars
                .get("HTTPS_PROXY")
                .and_then(|url| url.rsplit_once(':').map(|(_, port)| port))
                .filter(|port| port.parse::<u16>().is_ok())
                .ok_or_else(|| anyhow::anyhow!("--sandbox requires a localhost HTTPS_PROXY"))?;
            format!(
                "(deny network-inbound)\n            (deny network-outbound)\n            (allow network-outbound (remote ip \"localhost:{proxy_port}\"))"
            )
        } else {
            String::new()
        };

        // The agent can only make network connections to this command's loopback proxy.
        // `allow default` keeps language runtimes usable; direct outbound and inbound
        // sockets are then denied, with the proxy's exact port restored as the exception.
        let profile = format!(
            r#"
            (version 1)
            (allow default)
            {network_rules}
            {}
        "#,
            denied_process_exec_rules(env_vars, denied_commands)
                + &denied_file_rules(denied_read_paths, denied_write_paths)
        );
        return Ok((
            "/usr/bin/sandbox-exec".to_owned(),
            vec!["-p".to_owned(), profile, command.to_owned()],
        ));
    }

    #[cfg(target_os = "linux")]
    {
        let _ = denied_commands;
        if sandbox {
            if let Some(error) = systemd_filesystem_enforcement_error() {
                anyhow::bail!(error);
            }
        }
        if !sandbox
            && (!denied_read_paths.is_empty() || !denied_write_paths.is_empty())
            && systemd_filesystem_enforcement_error().is_some()
        {
            if let Some(error) = bubblewrap_enforcement_error() {
                anyhow::bail!(error);
            }
            return Ok(bubblewrap_command(
                command,
                denied_read_paths,
                denied_write_paths,
            ));
        }
        if !sandbox && denied_read_paths.is_empty() && denied_write_paths.is_empty() {
            return Ok((command.to_owned(), Vec::new()));
        }
        // systemd applies these cgroup IP rules only to the child command. The
        // parent-owned proxy remains outside the scope and can forward approved
        // requests to the internet, while the child can reach only 127.0.0.1.
        let mut args = vec![
            "--user".to_owned(),
            "--scope".to_owned(),
            "--quiet".to_owned(),
        ];
        if sandbox {
            args.extend([
                "--property=IPAddressDeny=any".to_owned(),
                "--property=IPAddressAllow=127.0.0.1".to_owned(),
                "--property=IPAddressAllow=::1".to_owned(),
            ]);
        }
        for path in resolve_policy_paths(denied_read_paths) {
            args.push(format!("--property=InaccessiblePaths={path}"));
        }
        for path in resolve_policy_paths(denied_write_paths) {
            args.push(format!("--property=ReadOnlyPaths={path}"));
        }
        args.extend(["--".to_owned(), command.to_owned()]);
        return Ok(("systemd-run".to_owned(), args));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("--sandbox is currently supported on macOS and systemd-based Linux")
    }
}

#[cfg(target_os = "macos")]
fn denied_process_exec_rules(
    env_vars: &HashMap<String, String>,
    denied_commands: &[String],
) -> String {
    let Some(path) = env_vars.get("PATH") else {
        return String::new();
    };
    let mut rules = Vec::new();
    for command in denied_commands {
        for directory in std::env::split_paths(path) {
            let candidate = directory.join(command);
            if candidate.is_file() {
                let escaped = candidate
                    .to_string_lossy()
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                rules.push(format!("(deny process-exec (literal \"{escaped}\"))"));
            }
        }
    }
    rules.sort();
    rules.dedup();
    rules.join("\n            ")
}

#[cfg(target_os = "macos")]
fn denied_file_rules(deny_read: &[String], deny_write: &[String]) -> String {
    let mut rules = Vec::new();
    for path in resolve_policy_paths(deny_read) {
        let escaped = escape_sbpl_path(&path);
        let matcher = if PathBuf::from(&path).is_file() {
            "literal"
        } else {
            "subpath"
        };
        rules.push(format!("(deny file-read* ({matcher} \"{escaped}\"))"));
    }
    for path in resolve_policy_paths(deny_write) {
        let escaped = escape_sbpl_path(&path);
        let matcher = if PathBuf::from(&path).is_file() {
            "literal"
        } else {
            "subpath"
        };
        rules.push(format!("(deny file-write* ({matcher} \"{escaped}\"))"));
    }
    rules.sort();
    rules.dedup();
    if rules.is_empty() {
        String::new()
    } else {
        format!("\n            {}", rules.join("\n            "))
    }
}

fn resolve_policy_paths(paths: &[String]) -> Vec<String> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut resolved = paths
        .iter()
        .map(|path| {
            let path = path.trim();
            let path = if path == "~" {
                home.clone().unwrap_or_else(|| PathBuf::from(path))
            } else if let Some(rest) = path.strip_prefix("~/") {
                home.clone()
                    .unwrap_or_else(|| PathBuf::from("~"))
                    .join(rest)
            } else {
                PathBuf::from(path)
            };
            if path.is_absolute() {
                path
            } else {
                current_dir.join(path)
            }
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    resolved.sort();
    resolved.dedup();
    resolved
}

#[cfg(unix)]
fn emit_child_stderr(
    stderr: &[u8],
    denied_read_paths: &[String],
    denied_write_paths: &[String],
    audit_log: Option<&super::proxy::ProxyAuditLog>,
) -> Result<()> {
    for line in String::from_utf8_lossy(stderr).lines() {
        if let Some((path, operation)) =
            filesystem_denial_from_line(line, denied_read_paths, denied_write_paths)
        {
            let id =
                audit_log.map(|audit_log| audit_log.record_filesystem_denied(&path, operation));
            println!(
                "{}",
                serde_json::json!({
                    "error": {
                        "code": "filesystem_denied",
                        "message": "Filesystem access denied by agent policy",
                        "path": path,
                        "operation": operation,
                        "id": id,
                    }
                })
            );
        } else {
            eprintln!("{line}");
        }
    }
    Ok(())
}

fn filesystem_denial_from_line(
    line: &str,
    denied_read_paths: &[String],
    denied_write_paths: &[String],
) -> Option<(String, &'static str)> {
    if !line.contains("Operation not permitted") && !line.contains("Permission denied") {
        return None;
    }

    let matches_path = |configured: &str| {
        let configured = configured.trim();
        let Some(denied_path) = resolve_policy_paths(&[configured.to_owned()])
            .into_iter()
            .next()
            .map(PathBuf::from)
        else {
            return false;
        };
        line.split_whitespace().any(|token| {
            let token = token
                .trim_matches(|character| matches!(character, '\'' | '"' | '(' | '[' | '{'))
                .trim_end_matches([':', ',', ')', ']', '}']);
            if token.is_empty() {
                return false;
            }
            let candidate = PathBuf::from(token);
            let candidate = if candidate.is_absolute() {
                candidate
            } else {
                env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(candidate)
            };
            candidate == denied_path || candidate.starts_with(&denied_path)
        })
    };

    let read_match = denied_read_paths.iter().find(|path| matches_path(path));
    let write_match = denied_write_paths.iter().find(|path| matches_path(path));
    match (read_match, write_match) {
        (Some(path), Some(_)) => Some((path.trim().to_owned(), "read/write")),
        (Some(path), None) => Some((path.trim().to_owned(), "read")),
        (None, Some(path)) => Some((path.trim().to_owned(), "write")),
        (None, None) => None,
    }
}

#[cfg(target_os = "macos")]
fn escape_sbpl_path(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "linux")]
fn command_in_path(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(command).is_file())
    })
}

pub(crate) fn systemd_run_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        systemd_filesystem_enforcement_error().is_none()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Returns the backend that will enforce a non-empty profile denylist.
pub(crate) fn filesystem_backend() -> String {
    #[cfg(target_os = "linux")]
    {
        if systemd_filesystem_enforcement_error().is_none() {
            return "systemd-run --user (denylist-compatible)".to_owned();
        }
        if bubblewrap_enforcement_error().is_none() {
            return "bubblewrap (denylist-compatible)".to_owned();
        }
        "unavailable (systemd-run and bubblewrap are unavailable)".to_owned()
    }
    #[cfg(target_os = "macos")]
    {
        "macOS Seatbelt (sandbox-exec)".to_owned()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        "unavailable".to_owned()
    }
}

pub(crate) fn filesystem_backend_for_policy(
    denied_read_paths: &[String],
    denied_write_paths: &[String],
) -> String {
    if denied_read_paths.is_empty() && denied_write_paths.is_empty() {
        "none (no filesystem deny rules)".to_owned()
    } else {
        filesystem_backend()
    }
}

#[cfg(target_os = "linux")]
fn bubblewrap_enforcement_error() -> Option<String> {
    if !command_in_path("bwrap") && !command_in_path("bubblewrap") {
        return Some(
            "filesystem restrictions cannot run here: neither `systemd-run --user` nor bubblewrap is available"
                .to_owned(),
        );
    }
    let executable = if command_in_path("bwrap") {
        "bwrap"
    } else {
        "bubblewrap"
    };
    let probe = std::process::Command::new(executable)
        .args([
            "--die-with-parent",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev-bind",
            "/dev",
            "/dev",
            "--",
            "/bin/true",
        ])
        .output();
    match probe {
        Ok(output) if output.status.success() => None,
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Some(format!(
                "filesystem restrictions cannot run here: bubblewrap could not create a usable mount namespace{}",
                if detail.is_empty() { String::new() } else { format!(": {detail}") }
            ))
        }
        Err(error) => Some(format!(
            "filesystem restrictions cannot run here: bubblewrap could not be probed: {error}"
        )),
    }
}

#[cfg(target_os = "linux")]
fn bubblewrap_command(
    command: &str,
    denied_read_paths: &[String],
    denied_write_paths: &[String],
) -> (String, Vec<String>) {
    let executable = if command_in_path("bwrap") {
        "bwrap"
    } else {
        "bubblewrap"
    };
    let mut args = vec![
        "--die-with-parent".to_owned(),
        "--bind".to_owned(),
        "/".to_owned(),
        "/".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev-bind".to_owned(),
        "/dev".to_owned(),
        "/dev".to_owned(),
        "--chdir".to_owned(),
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .to_string_lossy()
            .into_owned(),
    ];
    let read_paths = resolve_policy_paths(denied_read_paths);
    for path in &read_paths {
        if PathBuf::from(path).is_dir() {
            args.extend(["--tmpfs".to_owned(), path.clone()]);
        } else {
            args.extend(["--ro-bind".to_owned(), "/dev/null".to_owned(), path.clone()]);
        }
    }
    for path in resolve_policy_paths(denied_write_paths) {
        if read_paths.iter().any(|read| {
            let read = PathBuf::from(read);
            let path = PathBuf::from(&path);
            path == read || path.starts_with(read)
        }) {
            continue;
        }
        args.extend(["--ro-bind".to_owned(), path.clone(), path]);
    }
    args.extend(["--".to_owned(), command.to_owned()]);
    (executable.to_owned(), args)
}

#[cfg(target_os = "linux")]
fn systemd_filesystem_enforcement_error() -> Option<String> {
    if !command_in_path("systemd-run") {
        return Some("systemd-run is not available".to_owned());
    }
    let probe = std::process::Command::new("systemd-run")
        .args(["--user", "--scope", "--quiet", "--wait", "--", "/bin/true"])
        .output();
    match probe {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

/// Checks the backend used to enforce filesystem restrictions, including the
/// user service manager connection required by `systemd-run --user`.
pub(crate) fn filesystem_enforcement_error() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if systemd_filesystem_enforcement_error().is_none()
            || bubblewrap_enforcement_error().is_none()
        {
            None
        } else {
            Some(format!(
                "filesystem restrictions cannot run here: {}",
                filesystem_backend()
            ))
        }
    }
    #[cfg(target_os = "macos")]
    {
        None
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Some("filesystem restrictions cannot run here: no supported enforcement backend (expected macOS Seatbelt or Linux `systemd-run --user`)".to_owned())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        filesystem_backend_for_policy, filesystem_denial_from_line, run_command, sandbox_command,
    };
    #[cfg(target_os = "macos")]
    use super::{run_command_with_denied_commands, sandbox_command_with_denied_commands};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn parses_structured_filesystem_denial() {
        let denial = filesystem_denial_from_line(
            "cat: .env: Operation not permitted",
            &[".env".to_owned()],
            &[],
        );
        assert_eq!(denial, Some((".env".to_owned(), "read")));
    }

    #[test]
    fn backend_selection_does_not_claim_a_filesystem_backend_without_rules() {
        assert_eq!(
            filesystem_backend_for_policy(&[], &[]),
            "none (no filesystem deny rules)"
        );
    }

    #[test]
    fn ignores_unrelated_permission_errors_with_matching_basename() {
        let denial = filesystem_denial_from_line(
            "tool: /tmp/other/.env: Permission denied",
            &["/tmp/project/.env".to_owned()],
            &[],
        );
        assert_eq!(denial, None);
    }

    #[test]
    fn matches_normalized_relative_paths_and_denied_directory_descendants() {
        let relative = filesystem_denial_from_line(
            "cat: ./.env: Operation not permitted",
            &[".env".to_owned()],
            &[],
        );
        assert_eq!(relative, Some((".env".to_owned(), "read")));

        let descendant = filesystem_denial_from_line(
            "cat: /tmp/private-agent/keys/id_ed25519: Permission denied",
            &[],
            &["/tmp/private-agent".to_owned()],
        );
        assert_eq!(descendant, Some(("/tmp/private-agent".to_owned(), "write")));
    }

    #[test]
    fn classifies_exact_write_denial() {
        let denial = filesystem_denial_from_line(
            "echo: protected.txt: Permission denied",
            &[],
            &["protected.txt".to_owned()],
        );
        assert_eq!(denial, Some(("protected.txt".to_owned(), "write")));
    }

    #[test]
    fn does_not_match_sibling_directory_with_shared_prefix() {
        let denial = filesystem_denial_from_line(
            "cat: /tmp/private-agent-other/key: Operation not permitted",
            &["/tmp/private-agent".to_owned()],
            &[],
        );
        assert_eq!(denial, None);
    }

    #[test]
    fn ignores_permission_error_without_configured_path() {
        let denial = filesystem_denial_from_line(
            "cat: public.txt: Permission denied",
            &[".env".to_owned()],
            &[".git".to_owned()],
        );
        assert_eq!(denial, None);
    }

    #[tokio::test]
    async fn returns_the_child_exit_status() {
        let status = run_command(
            "sh",
            vec!["-c".to_owned(), "exit 7".to_owned()],
            HashMap::new(),
            Vec::new(),
            false,
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(status.code(), Some(7));
    }

    #[tokio::test]
    async fn denied_commands_are_shadowed_for_child_processes() {
        let status = super::run_command_with_denied_commands(
            "sh",
            vec!["-c".to_owned(), "blocked-tool".to_owned()],
            HashMap::new(),
            Vec::new(),
            false,
            false,
            false,
            &["blocked-tool".to_owned()],
            &[],
            &[],
            None,
        )
        .await
        .unwrap();

        assert_eq!(status.code(), Some(126));
    }

    #[tokio::test]
    async fn restricted_child_does_not_inherit_stashbase_api_key() {
        let _guard = environment_lock().lock().unwrap();
        let previous = std::env::var_os("STASHBASE_API_KEY");
        std::env::set_var("STASHBASE_API_KEY", "parent-api-key");

        let status = run_command(
            "sh",
            vec!["-c".to_owned(), "test -z \"$STASHBASE_API_KEY\"".to_owned()],
            HashMap::new(),
            Vec::new(),
            false,
            false,
            true,
        )
        .await
        .unwrap();

        match previous {
            Some(value) => std::env::set_var("STASHBASE_API_KEY", value),
            None => std::env::remove_var("STASHBASE_API_KEY"),
        }
        assert!(status.success());
    }

    #[tokio::test]
    async fn proxy_child_removes_the_source_name_when_a_secret_is_renamed() {
        let _guard = environment_lock().lock().unwrap();
        let previous = std::env::var_os("GITHUB_TOKEN");
        std::env::set_var("GITHUB_TOKEN", "parent-token");

        let status = run_command(
            "sh",
            vec![
                "-c".to_owned(),
                "test -z \"$GITHUB_TOKEN\" && test \"$GH_TOKEN\" = \"proxy-placeholder\""
                    .to_owned(),
            ],
            HashMap::from([("GH_TOKEN".to_owned(), "proxy-placeholder".to_owned())]),
            vec!["GITHUB_TOKEN".to_owned()],
            false,
            true,
            true,
        )
        .await
        .unwrap();

        match previous {
            Some(value) => std::env::set_var("GITHUB_TOKEN", value),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
        assert!(status.success());
    }

    #[tokio::test]
    async fn proxy_child_clears_proxy_bypass_overrides() {
        let _guard = environment_lock().lock().unwrap();
        let saved = [
            ("NO_PROXY", std::env::var_os("NO_PROXY")),
            ("ALL_PROXY", std::env::var_os("ALL_PROXY")),
            ("npm_config_proxy", std::env::var_os("npm_config_proxy")),
        ];
        std::env::set_var("NO_PROXY", "api.example.com");
        std::env::set_var("ALL_PROXY", "http://other-proxy.invalid");
        std::env::set_var("npm_config_proxy", "http://other-proxy.invalid");

        let status = run_command(
            "sh",
            vec![
                "-c".to_owned(),
                "test -z \"$NO_PROXY\" && test -z \"$ALL_PROXY\" && test -z \"$npm_config_proxy\""
                    .to_owned(),
            ],
            HashMap::from([
                ("HTTP_PROXY".to_owned(), "http://127.0.0.1:9999".to_owned()),
                ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:9999".to_owned()),
                ("NO_PROXY".to_owned(), String::new()),
                ("no_proxy".to_owned(), String::new()),
            ]),
            Vec::new(),
            false,
            true,
            false,
        )
        .await
        .unwrap();

        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        assert!(status.success());
    }

    #[tokio::test]
    async fn profile_managed_anthropic_key_overrides_local_auth_variables() {
        let _guard = environment_lock().lock().unwrap();
        let saved = [
            ("ANTHROPIC_API_KEY", std::env::var_os("ANTHROPIC_API_KEY")),
            (
                "ANTHROPIC_AUTH_TOKEN",
                std::env::var_os("ANTHROPIC_AUTH_TOKEN"),
            ),
            (
                "CLAUDE_CODE_OAUTH_TOKEN",
                std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN"),
            ),
        ];
        std::env::set_var("ANTHROPIC_API_KEY", "local-api-key");
        std::env::set_var("ANTHROPIC_AUTH_TOKEN", "local-auth-token");
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "local-oauth-token");

        let status = run_command(
            "sh",
            vec![
                "-c".to_owned(),
                "test \"$ANTHROPIC_API_KEY\" = \"proxy-placeholder\" && test -z \"$ANTHROPIC_AUTH_TOKEN\" && test -z \"$CLAUDE_CODE_OAUTH_TOKEN\"".to_owned(),
            ],
            HashMap::from([(
                "ANTHROPIC_API_KEY".to_owned(),
                "proxy-placeholder".to_owned(),
            )]),
            Vec::new(),
            false,
            true,
            false,
        )
        .await
        .unwrap();

        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        assert!(status.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_sandbox_allows_only_the_configured_proxy_port() {
        use std::{net::TcpListener, process::Command, thread};

        let allowed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let allowed_port = allowed_listener.local_addr().unwrap().port();
        let accepted = thread::spawn(move || allowed_listener.accept().is_ok());
        let env_vars = HashMap::from([(
            "HTTPS_PROXY".to_owned(),
            format!("http://127.0.0.1:{allowed_port}"),
        )]);
        let (program, launcher_args) = sandbox_command("/usr/bin/nc", true, &env_vars).unwrap();
        let allowed_port = allowed_port.to_string();

        let allowed = Command::new(&program)
            .args(&launcher_args)
            .args(["-z", "127.0.0.1", &allowed_port])
            .status()
            .unwrap();
        assert!(allowed.success());
        assert!(accepted.join().unwrap());

        let denied_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let denied_port = denied_listener.local_addr().unwrap().port().to_string();
        let denied = Command::new(program)
            .args(launcher_args)
            .args(["-z", "127.0.0.1", &denied_port])
            .status()
            .unwrap();
        assert!(!denied.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_sandbox_denies_blocked_executables_by_absolute_path() {
        let env_vars = HashMap::from([
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            (
                "HTTPS_PROXY".to_owned(),
                "http://127.0.0.1:49152".to_owned(),
            ),
        ]);
        let (_, args) = sandbox_command_with_denied_commands(
            "/bin/sh",
            true,
            &env_vars,
            &["curl".to_owned()],
            &[],
            &[],
        )
        .unwrap();
        let profile = &args[1];
        assert!(profile.contains("(deny process-exec (literal \"/usr/bin/curl\"))"));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_command_policy_blocks_absolute_child_execution_without_network_sandbox() {
        let status = run_command_with_denied_commands(
            "/bin/sh",
            vec!["-c".to_owned(), "/usr/bin/curl --version".to_owned()],
            HashMap::new(),
            Vec::new(),
            false,
            false,
            false,
            &["curl".to_owned()],
            &[],
            &[],
            None,
        )
        .await
        .unwrap();

        assert!(!status.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_filesystem_policy_generates_file_read_rule() {
        let (program, args) = sandbox_command_with_denied_commands(
            "/bin/sh",
            false,
            &HashMap::new(),
            &[],
            &["/tmp/private-agent-file".to_owned()],
            &[],
        )
        .unwrap();

        assert_eq!(program, "/usr/bin/sandbox-exec");
        assert!(args[1].contains("(deny file-read* (subpath \"/tmp/private-agent-file\"))"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_sandbox_uses_systemd_cgroup_network_rules() {
        let env_vars = HashMap::from([(
            "HTTPS_PROXY".to_owned(),
            "http://127.0.0.1:49152".to_owned(),
        )]);
        let (program, args) = sandbox_command("curl", true, &env_vars).unwrap();
        assert_eq!(program, "systemd-run");
        assert!(args.contains(&"--property=IPAddressDeny=any".to_owned()));
        assert!(args.contains(&"--property=IPAddressAllow=127.0.0.1".to_owned()));
        assert_eq!(args.last(), Some(&"curl".to_owned()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bubblewrap_maps_read_and_write_denies_to_mounts() {
        let (program, args) = super::bubblewrap_command(
            "sh",
            &[
                "/tmp/private-dir".to_owned(),
                "/tmp/private-file".to_owned(),
            ],
            &[
                "/tmp/readonly-dir".to_owned(),
                "/tmp/readonly-file".to_owned(),
            ],
        );
        assert!(program == "bwrap" || program == "bubblewrap");
        assert!(args
            .windows(2)
            .any(|window| { window == ["--tmpfs", "/tmp/private-dir"] }));
        assert!(args
            .windows(3)
            .any(|window| { window == ["--ro-bind", "/dev/null", "/tmp/private-file"] }));
        assert!(args
            .windows(3)
            .any(|window| { window == ["--ro-bind", "/tmp/readonly-file", "/tmp/readonly-file"] }));
    }
}
