//! Static validation for agent profiles.
//!
//! This is deliberately local-only: no secret source is read and no proxy is
//! started. It lets a repository check its profile in CI before an agent gets
//! access to any credential source.

use std::{
    collections::{HashMap, HashSet},
    fs,
    net::IpAddr,
    path::Path,
};

use anyhow::Result;
use hyper::header::HeaderName;
use serde::Serialize;

use crate::{
    cmd::agent::{AgentProfileSource, AgentValidateCommand},
    config::config,
    models::{agent::AgentProfile, config::Config},
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
    profile: String,
    status: Status,
    checks: Vec<Check>,
}

pub async fn handle_agent_validate_command(
    command: AgentValidateCommand,
    global_config: &Config,
    json_format: bool,
) -> Result<bool> {
    let mut checks = Vec::new();
    let global_profile = global_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let (profile, directory_profile) = match command.profile_source {
        AgentProfileSource::Global => (global_profile, false),
        AgentProfileSource::Directory => {
            (config::get_directory_agent_profile(&command.profile)?, true)
        }
        AgentProfileSource::Auto => {
            let directory_profile = config::get_directory_agent_profile(&command.profile)?;
            let from_directory = directory_profile.is_some();
            (directory_profile.or(global_profile), from_directory)
        }
    };

    let Some(profile) = profile else {
        checks.push(fail(
            "Profile",
            format!(
                "Profile '{}' was not found in the {} config.",
                command.profile,
                source_label(command.profile_source)
            ),
        ));
        return print_report(command.profile, checks, json_format);
    };

    checks.push(ok(
        "Profile source",
        if directory_profile {
            "Loaded from ./stashbase-agent.toml".to_owned()
        } else {
            "Loaded from user-level config".to_owned()
        },
    ));
    if directory_profile && matches!(command.profile_source, AgentProfileSource::Auto) {
        checks.push(warn(
            "Repository policy",
            "Auto selected ./stashbase-agent.toml. Review this repository-controlled policy before granting secrets."
                .to_owned(),
        ));
    }

    checks.extend(validate_profile(&profile));
    if command.remote {
        checks.extend(validate_remote_profile(&profile));
    }
    print_report(command.profile, checks, json_format)
}

/// Enforces the static profile checks before an agent is started. Keeping this
/// alongside `agent validate` ensures a user cannot accidentally bypass its
/// safety and determinism checks by invoking `agent run` directly.
pub fn ensure_profile_is_valid_for_run(profile: &AgentProfile) -> Result<()> {
    let failures = validate_profile(profile)
        .into_iter()
        .filter(|check| check.status == Status::Fail)
        .map(|check| format!("{}: {}", check.name, check.message))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        return Ok(());
    }

    anyhow::bail!(
        "Agent profile is invalid:\n{}",
        failures
            .into_iter()
            .map(|failure| format!("- {failure}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn validate_remote_profile(profile: &AgentProfile) -> Vec<Check> {
    let mut checks = Vec::new();
    if profile.file.is_some()
        || profile.secrets.is_empty()
        || profile.project.is_none()
        || profile.environment.is_none()
    {
        checks.push(fail(
            "Remote session profile",
            "--remote requires project/environment-backed secret bindings and does not support local-file or egress-only profiles."
                .to_owned(),
        ));
    } else {
        checks.push(ok(
            "Remote session profile",
            "Project/environment-backed secret bindings are compatible with --remote.".to_owned(),
        ));
    }

    match crate::handlers::run::proxy::cached_remote_proxy_ca_files() {
        Ok(paths) if !paths.is_empty() => checks.push(ok(
            "Remote agent proxy CA",
            format!("Found {} valid cached CA file(s).", paths.len()),
        )),
        Ok(_) => checks.push(warn(
            "Remote agent proxy CA",
            "Not cached yet; Stashbase will provision it from the authenticated remote session on first run."
                .to_owned(),
        )),
        Err(error) => checks.push(warn(
            "Remote agent proxy CA",
            format!("Cached CA is invalid and will be refreshed on the next remote run: {error}"),
        )),
    }
    checks
}

fn print_report(profile: String, checks: Vec<Check>, json_format: bool) -> Result<bool> {
    let status = overall_status(&checks);
    let report = Report {
        profile: profile.clone(),
        status,
        checks,
    };
    if json_format {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("Stashbase Agent Profile Validation: `{profile}`\n");
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
                Status::Ok => "Profile validation passed.".green_if_tty(),
                Status::Warn => "Profile validation passed with warnings.".yellow_if_tty(),
                Status::Fail => "Profile validation failed.".red_if_tty(),
            }
        );
    }
    Ok(report.status == Status::Fail)
}

fn validate_profile(profile: &AgentProfile) -> Vec<Check> {
    let mut checks = Vec::new();
    let egress_only = profile.secrets.is_empty();
    let valid_source = matches!(
        (&profile.file, &profile.project, &profile.environment),
        (Some(_), None, None) | (None, Some(_), Some(_)) | (Some(_), Some(_), Some(_))
    );
    if egress_only {
        if matches!(
            (&profile.file, &profile.project, &profile.environment),
            (None, None, None)
        ) {
            checks.push(ok(
                "Profile mode",
                "Egress-only: no Stashbase-managed secrets or secret source are configured."
                    .to_owned(),
            ));
        } else {
            checks.push(fail(
                "Profile mode",
                "An egress-only profile must not define 'file', 'project', or 'environment'."
                    .to_owned(),
            ));
        }
    } else if valid_source {
        checks.push(ok(
            "Secret source",
            match (&profile.file, &profile.project) {
                (Some(file), Some(_)) => format!("Local file '{file}' with remote fallback"),
                (Some(file), None) => format!("Local file '{file}'"),
                (None, Some(_)) => "Remote project and environment".to_owned(),
                _ => unreachable!(),
            },
        ));
    } else {
        checks.push(fail(
            "Secret source",
            "Set 'file', both 'project' and 'environment', or both sources together.".to_owned(),
        ));
    }

    for (name, value) in [
        ("project", profile.project.as_deref()),
        ("environment", profile.environment.as_deref()),
        ("file", profile.file.as_deref()),
    ] {
        if value.is_some_and(|value| value.trim().is_empty()) {
            checks.push(fail(
                format!("{name} value"),
                "Must not be empty or whitespace.".to_owned(),
            ));
        }
    }
    if let Some(file) = &profile.file {
        let path = Path::new(file);
        if !path.is_file() {
            checks.push(fail(
                "Local secret file",
                format!("File not found: {}", path.display()),
            ));
        } else if fs::File::open(path).is_err() {
            checks.push(fail(
                "Local secret file",
                format!("File is not readable: {}", path.display()),
            ));
        } else {
            checks.push(ok(
                "Local secret file",
                format!("Readable: {}", path.display()),
            ));
        }
    }

    let mut bindings: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut child_envs: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut placeholders: HashMap<&str, Vec<&str>> = HashMap::new();
    for (target, secret) in &profile.secrets {
        let env = secret.env.as_deref().unwrap_or(target);
        child_envs.entry(env).or_default().push(target);
        if !valid_environment_name(env) {
            checks.push(fail(
                format!("Secret binding '{target}'"),
                "The child environment variable name must use letters, digits, and underscores and cannot start with a digit."
                    .to_owned(),
            ));
        }
        if secret
            .env
            .as_deref()
            .is_some_and(|env| env.trim().is_empty())
        {
            checks.push(fail(
                format!("Secret binding '{target}' env"),
                "'env' must not be empty when set.".to_owned(),
            ));
        }
        if secret.placeholder.as_deref().is_some_and(|placeholder| {
            placeholder.trim().is_empty() || placeholder.contains(['\r', '\n'])
        }) {
            checks.push(fail(
                format!("Secret binding '{target}' placeholder"),
                "'placeholder' must not be empty or contain a line break.".to_owned(),
            ));
        }
        if let Some(placeholder) = secret.placeholder.as_deref() {
            placeholders.entry(placeholder).or_default().push(target);
        }
        let source = secret.from.as_deref().unwrap_or(target);
        if source.trim().is_empty() {
            checks.push(fail(
                format!("Secret binding '{target}'"),
                "'from' must not be empty when set.".to_owned(),
            ));
        }
        bindings.entry(source).or_default().push(target);

        if secret.hosts.is_empty() {
            checks.push(fail(
                format!("Secret '{target}' hosts"),
                "At least one destination host is required for credential injection.".to_owned(),
            ));
        }
        let mut seen_hosts = HashSet::new();
        for host in &secret.hosts {
            if let Err(reason) = validate_host(host, false) {
                checks.push(fail(format!("Secret '{target}' host"), reason));
            } else if !seen_hosts.insert(host.trim().trim_end_matches('.').to_ascii_lowercase()) {
                checks.push(warn(
                    format!("Secret '{target}' host"),
                    format!("Duplicate destination host '{host}'."),
                ));
            }
        }
        if let Some(header) = &secret.header {
            if HeaderName::from_bytes(header.as_bytes()).is_err() {
                checks.push(fail(
                    format!("Secret '{target}' header"),
                    format!("'{header}' is not a valid HTTP header name."),
                ));
            }
        }
        if secret
            .value_template
            .as_deref()
            .is_some_and(|template| !template.contains("{secret}"))
        {
            checks.push(fail(
                format!("Secret '{target}' value_template"),
                "Must contain '{secret}' so the proxy can inject the credential.".to_owned(),
            ));
        }
    }
    for (source, targets) in bindings {
        if targets.len() > 1 {
            checks.push(fail(
                "Secret bindings",
                format!(
                    "Source secret '{source}' is bound to more than one child variable: {}.",
                    targets.join(", ")
                ),
            ));
        }
    }
    for (env, targets) in child_envs {
        if targets.len() > 1 {
            checks.push(fail(
                "Secret bindings",
                format!(
                    "Child environment variable '{env}' is used by more than one binding: {}.",
                    targets.join(", ")
                ),
            ));
        }
    }
    for (placeholder, targets) in placeholders {
        if targets.len() > 1 {
            checks.push(fail(
                "Secret bindings",
                format!(
                    "Placeholder '{placeholder}' is used by more than one binding: {}.",
                    targets.join(", ")
                ),
            ));
        }
    }

    let mut seen_egress = HashSet::new();
    for host in profile.egress_hosts.as_deref().unwrap_or_default() {
        if let Err(reason) = validate_host(host, true) {
            checks.push(fail("Egress host", reason));
        } else if host == "*" {
            checks.push(warn(
                "Egress host",
                "'*' allows non-credential traffic to every destination.".to_owned(),
            ));
        } else if !seen_egress.insert(host.trim().trim_end_matches('.').to_ascii_lowercase()) {
            checks.push(warn(
                "Egress host",
                format!("Duplicate egress host '{host}'."),
            ));
        }
    }

    let mut seen_denied = HashSet::new();
    for host in profile.deny_hosts.as_deref().unwrap_or_default() {
        if let Err(reason) = validate_host(host, true) {
            checks.push(fail("Denied host", reason));
        } else if host == "*" {
            checks.push(warn(
                "Denied host",
                "'*' blocks every destination, including configured secret hosts.".to_owned(),
            ));
        } else if !seen_denied.insert(host.trim().trim_end_matches('.').to_ascii_lowercase()) {
            checks.push(warn(
                "Denied host",
                format!("Duplicate denied host '{host}'."),
            ));
        }
    }

    if !checks.iter().any(|check| check.status == Status::Fail) {
        checks.push(ok(
            "Profile policy",
            format!(
                "{} secret binding(s), {} egress host rule(s), and {} denied host rule(s) are valid.",
                profile.secrets.len(),
                profile.egress_hosts.as_ref().map_or(0, Vec::len),
                profile.deny_hosts.as_ref().map_or(0, Vec::len)
            ),
        ));
    }
    checks
}

fn validate_host(host: &str, allow_all: bool) -> std::result::Result<(), String> {
    if host.trim() != host || host.is_empty() {
        return Err(format!(
            "'{host}' must be a non-empty host without whitespace."
        ));
    }
    if host == "*" {
        return allow_all.then_some(()).ok_or_else(|| {
            "'*' is allowed only in egress_hosts or deny_hosts, never for a secret.".to_owned()
        });
    }
    let host = host.trim_end_matches('.');
    let host = host.strip_prefix("*.").unwrap_or(host);
    if host.is_empty() || host.contains(['/', ':', '@']) {
        return Err("Use a hostname or '*.example.com', without a URL, port, or path.".to_owned());
    }
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if host.split('.').all(valid_host_label) {
        Ok(())
    } else {
        Err("Use a hostname or '*.example.com'.".to_owned())
    }
}

fn valid_host_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(byte) if byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn source_label(source: AgentProfileSource) -> &'static str {
    match source {
        AgentProfileSource::Global => "global",
        AgentProfileSource::Directory => "directory",
        AgentProfileSource::Auto => "global or directory",
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
    fn accepts_exact_hosts_and_subdomain_wildcards() {
        assert!(validate_host("api.github.com", false).is_ok());
        assert!(validate_host("*.githubcopilot.com", false).is_ok());
        assert!(validate_host("127.0.0.1", false).is_ok());
    }

    #[test]
    fn rejects_unsafe_secret_host_rules() {
        assert!(validate_host("*", false).is_err());
        assert!(validate_host("https://api.example.com", false).is_err());
        assert!(validate_host("api.example.com:443", false).is_err());
    }

    #[test]
    fn requires_templates_to_contain_the_secret_marker() {
        let profile = AgentProfile {
            project: Some("project".to_owned()),
            environment: Some("development".to_owned()),
            file: None,
            egress_hosts: None,
            deny_hosts: None,
            secrets: HashMap::from([(
                "API_KEY".to_owned(),
                crate::models::agent::AgentSecretProfile {
                    hosts: vec!["api.example.com".to_owned()],
                    from: None,
                    env: None,
                    placeholder: None,
                    header: Some("x-api-key".to_owned()),
                    value_template: Some("static-value".to_owned()),
                },
            )]),
        };
        assert!(validate_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail && check.name.contains("value_template")));
    }

    #[test]
    fn rejects_an_egress_only_profile_with_a_secret_source() {
        let profile = AgentProfile {
            project: None,
            environment: None,
            file: Some(".env.agent".to_owned()),
            egress_hosts: Some(vec!["chatgpt.com".to_owned()]),
            deny_hosts: None,
            secrets: HashMap::new(),
        };

        assert!(validate_profile(&profile).iter().any(|check| {
            check.status == Status::Fail && check.message.contains("egress-only profile")
        }));
    }

    #[test]
    fn run_validation_rejects_duplicate_remote_placeholders() {
        let profile = AgentProfile {
            project: Some("project".to_owned()),
            environment: Some("development".to_owned()),
            file: None,
            egress_hosts: None,
            deny_hosts: None,
            secrets: HashMap::from([
                (
                    "FIRST_KEY".to_owned(),
                    crate::models::agent::AgentSecretProfile {
                        hosts: vec!["first.example.com".to_owned()],
                        from: None,
                        env: None,
                        placeholder: Some("shared-placeholder".to_owned()),
                        header: None,
                        value_template: None,
                    },
                ),
                (
                    "SECOND_KEY".to_owned(),
                    crate::models::agent::AgentSecretProfile {
                        hosts: vec!["second.example.com".to_owned()],
                        from: None,
                        env: None,
                        placeholder: Some("shared-placeholder".to_owned()),
                        header: None,
                        value_template: None,
                    },
                ),
            ]),
        };

        let error = ensure_profile_is_valid_for_run(&profile).unwrap_err();
        assert!(error
            .to_string()
            .contains("Placeholder 'shared-placeholder'"));
    }
}
