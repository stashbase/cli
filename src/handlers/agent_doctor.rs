//! Read-only diagnostics for tools launched by `stashbase agent run`.
//!
//! This intentionally does not load a profile or secret. It verifies the local
//! broker setup and reports the proxy/TLS behavior Stashbase can reasonably
//! expect from a known executable. A third-party tool's actual network request
//! remains its responsibility, so users should still run an end-to-end profile
//! test before relying on a new tool.

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::Serialize;

use crate::{
    cmd::agent::AgentDoctorCommand,
    handlers::run::broker::{Broker, BrokerPolicy},
    utils::output::{get_formatted_json_string, ColorizeIfColoredOutput},
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    status: Status,
    message: String,
}

#[derive(Debug, Serialize)]
struct Report {
    tool: String,
    status: Status,
    checks: Vec<Check>,
}

pub async fn handle_agent_doctor_command(
    command: AgentDoctorCommand,
    json_format: bool,
) -> Result<bool> {
    let mut checks = Vec::new();
    let tool = command.tool;

    match command_path(&tool) {
        Some(path) => checks.push(ok(
            format!("Executable `{tool}`"),
            format!("Found at {}", path.display()),
        )),
        None => checks.push(fail(
            format!("Executable `{tool}`"),
            "Not found in PATH. Install it or pass its executable name.".to_owned(),
        )),
    }

    match Broker::start_with_port(HashMap::new(), BrokerPolicy::permissive(), None, None).await {
        Ok(broker) => {
            let child_env = broker.child_env();
            let proxy_ready = child_env.contains_key("HTTP_PROXY")
                && child_env.contains_key("HTTPS_PROXY")
                && child_env.get("NO_PROXY").is_some_and(String::is_empty)
                && child_env.get("no_proxy").is_some_and(String::is_empty);
            let tls_ready = [
                "SSL_CERT_FILE",
                "CURL_CA_BUNDLE",
                "GIT_SSL_CAINFO",
                "NODE_EXTRA_CA_CERTS",
            ]
            .iter()
            .all(|name| child_env.contains_key(*name));

            if proxy_ready {
                checks.push(ok(
                    "Proxy environment",
                    "HTTP_PROXY and HTTPS_PROXY are set; NO_PROXY is cleared for the child."
                        .to_owned(),
                ));
            } else {
                checks.push(fail(
                    "Proxy environment",
                    "The temporary broker did not provide a complete proxy environment.".to_owned(),
                ));
            }

            if tls_ready {
                checks.push(ok(
                    "HTTPS trust configuration",
                    "Temporary CA paths are configured for curl, Git, OpenSSL, and Node."
                        .to_owned(),
                ));
            } else {
                checks.push(fail(
                    "HTTPS trust configuration",
                    "The temporary broker did not provide all expected CA environment variables."
                        .to_owned(),
                ));
            }

            checks.push(ok(
                "Temporary broker",
                "Started and accepted its localhost proxy configuration.".to_owned(),
            ));
            broker.stop().await;
        }
        Err(error) => checks.push(fail(
            "Temporary broker",
            format!("Could not start a localhost broker: {error}"),
        )),
    }

    let normalized_tool = Path::new(&tool)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&tool)
        .to_ascii_lowercase();
    checks.push(tool_compatibility(&normalized_tool));
    checks.push(ok(
        "Scope of this check",
        "This verifies Stashbase's local broker setup and known tool support; run a real request with an allowed profile host to verify a tool's own network behavior."
            .to_owned(),
    ));

    let status = overall_status(&checks);
    let report = Report {
        tool: tool.clone(),
        status,
        checks,
    };

    if json_format {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("Stashbase Agent Doctor: `{tool}`\n");
        for check in &report.checks {
            let label = match check.status {
                Status::Ok => "OK".green_if_tty(),
                Status::Warn => "WARN".yellow_if_tty(),
                Status::Fail => "FAIL".red_if_tty(),
            };
            println!("[{label}] {}: {}", check.name, check.message);
        }
        println!();
        println!(
            "{}",
            match report.status {
                Status::Ok => "Agent doctor finished: all checks passed.".green_if_tty(),
                Status::Warn => "Agent doctor finished: warnings detected.".yellow_if_tty(),
                Status::Fail => "Agent doctor finished: failures detected.".red_if_tty(),
            }
        );
    }

    Ok(report.status == Status::Fail)
}

fn tool_compatibility(tool: &str) -> Check {
    match tool {
        "curl" => ok(
            "Tool compatibility",
            "curl supports HTTP(S) proxy variables and HTTPS CONNECT; the supplied CA bundle enables broker TLS interception."
                .to_owned(),
        ),
        "gh" => ok(
            "Tool compatibility",
            "GitHub CLI uses Go's HTTP proxy support, including HTTPS CONNECT."
                .to_owned(),
        ),
        "node" => ok(
            "Tool compatibility",
            "Node built-in fetch is supported: Stashbase sets NODE_USE_ENV_PROXY=1 and NODE_EXTRA_CA_CERTS."
                .to_owned(),
        ),
        "copilot" => ok(
            "Tool compatibility",
            "Copilot CLI is supported when its GitHub credential has Copilot access; end-to-end behavior can vary by CLI release."
                .to_owned(),
        ),
        "python" | "python3" => warn(
            "Tool compatibility",
            "Python libraries vary: requests and httpx normally honor proxy variables, but a library can opt out."
                .to_owned(),
        ),
        "codex" => ok(
            "Tool compatibility",
            "Codex is supported over HTTP/1 WebSocket upgrades as well as ordinary HTTP(S). Configure every required Codex host and use --trust-broker-ca if its installed release requires OS trust-store integration."
                .to_owned(),
        ),
        _ => warn(
            "Tool compatibility",
            "No built-in compatibility profile for this tool. It must honor HTTP_PROXY/HTTPS_PROXY and trust the temporary CA for HTTPS requests."
                .to_owned(),
        ),
    }
}

fn command_path(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return is_executable_file(path).then(|| path.to_path_buf());
    }
    let path_var = env::var_os("PATH")?;
    for directory in env::split_paths(&path_var) {
        for candidate in binary_candidates(command) {
            let path = directory.join(candidate);
            if is_executable_file(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn binary_candidates(command: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let mut candidates = vec![command.to_owned()];
        if let Some(extensions) = env::var_os("PATHEXT") {
            candidates.extend(
                extensions
                    .to_string_lossy()
                    .split(';')
                    .filter(|extension| !extension.is_empty())
                    .map(|extension| format!("{command}{extension}")),
            );
        }
        candidates
    }
    #[cfg(not(windows))]
    {
        vec![command.to_owned()]
    }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn overall_status(checks: &[Check]) -> Status {
    if checks.iter().any(|check| check.status == Status::Fail) {
        Status::Fail
    } else if checks.iter().any(|check| check.status == Status::Warn) {
        Status::Warn
    } else {
        Status::Ok
    }
}

fn ok(name: impl Into<String>, message: String) -> Check {
    Check {
        name: name.into(),
        status: Status::Ok,
        message,
    }
}

fn warn(name: impl Into<String>, message: String) -> Check {
    Check {
        name: name.into(),
        status: Status::Warn,
        message,
    }
}

fn fail(name: impl Into<String>, message: String) -> Check {
    Check {
        name: name.into(),
        status: Status::Fail,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_known_node_support() {
        let check = tool_compatibility("node");
        assert_eq!(check.status, Status::Ok);
        assert!(check.message.contains("NODE_USE_ENV_PROXY"));
    }

    #[test]
    fn warns_for_unknown_tools_without_claiming_support() {
        let check = tool_compatibility("custom-client");
        assert_eq!(check.status, Status::Warn);
        assert!(check.message.contains("No built-in compatibility profile"));
    }
}
