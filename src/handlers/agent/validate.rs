//! Static validation for agent profiles.
//!
//! This is deliberately local-only: no secret source is read and no proxy is
//! started. It lets a repository check its profile in CI before an agent gets
//! access to any credential source.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
    handlers::agent::env_check,
    models::{
        agent::{
            AgentFieldCondition, AgentHttpRule, AgentHttpRuleEffect, AgentMcpServer, AgentProfile,
            AgentScalar,
        },
        config::Config,
    },
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
    let explicit_profile = command
        .policy_file
        .as_deref()
        .map(config::get_explicit_agent_profile)
        .transpose()?;
    let global_profile = global_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let (profile, directory_source) = if let Some(profile) = explicit_profile {
        (Some(profile.profile), Some(profile.source))
    } else {
        match command.profile_source {
            AgentProfileSource::Global => (global_profile, None),
            AgentProfileSource::Directory => {
                let profile = config::get_directory_agent_profile(&command.profile)?;
                let source = profile.as_ref().map(|profile| profile.source.clone());
                (profile.map(|profile| profile.profile), source)
            }
            AgentProfileSource::Auto => {
                let directory_profile = config::get_directory_agent_profile(&command.profile)?;
                let directory_source = directory_profile
                    .as_ref()
                    .map(|profile| profile.source.clone());
                (
                    directory_profile
                        .map(|profile| profile.profile)
                        .or(global_profile),
                    directory_source,
                )
            }
        }
    };
    let directory_profile = directory_source.is_some();

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
            format!("Loaded from {}", directory_source.as_deref().unwrap())
        } else {
            "Loaded from user-level config".to_owned()
        },
    ));
    if directory_profile && matches!(command.profile_source, AgentProfileSource::Auto) {
        checks.push(warn(
            "Repository policy",
            "Auto selected a repository-local agent profile. Review this repository-controlled policy before granting secrets."
                .to_owned(),
        ));
    }

    checks.extend(validate_profile(&profile));
    checks.extend(validate_hook_capabilities(&profile));
    checks.extend(validate_config_env(&profile));
    checks.extend(validate_runtime_requirements(&profile));
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
        .chain(validate_hook_capabilities(profile))
        .chain(validate_config_env(profile))
        .chain(validate_runtime_requirements(profile))
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

fn validate_runtime_requirements(profile: &AgentProfile) -> Vec<Check> {
    let mut checks = Vec::new();

    // A Docker-backend profile never touches the native
    // Seatbelt/systemd-run/bubblewrap mechanisms the checks below test —
    // it's contained by the Docker sandbox instead, which is checked
    // separately. Reporting a native-backend failure for a profile that
    // will never use the native backend is actively misleading: it's the
    // difference between "this profile can't run here" (true) and "this
    // profile can't run *natively* here" (irrelevant to a Docker-backend
    // profile, and would wrongly report Windows as unsupported even though
    // Docker Desktop makes this backend work there too).
    if profile.sandbox.backend == crate::models::agent::SandboxBackend::Docker {
        match crate::handlers::run::docker_sandbox::docker_enforcement_error() {
            Some(error) => checks.push(fail("Docker sandbox backend", error)),
            None => checks.push(ok(
                "Docker sandbox backend",
                "Docker is installed and the daemon is reachable.".to_owned(),
            )),
        }
        return checks;
    }

    match crate::handlers::run::subprocess::network_enforcement_error() {
        Some(error) => checks.push(fail("Network enforcement", error)),
        None => checks.push(ok(
            "Network enforcement",
            "Loopback-only agent containment is available".to_owned(),
        )),
    }

    let backend = crate::handlers::run::subprocess::filesystem_backend_for_policy(
        &profile.filesystem.deny_read,
        &profile.filesystem.deny_write,
    );
    if profile.filesystem.deny_read.is_empty() && profile.filesystem.deny_write.is_empty() {
        checks.push(ok(
            "Filesystem enforcement",
            format!("Selected backend: {backend}."),
        ));
        return checks;
    }

    match crate::handlers::run::subprocess::filesystem_enforcement_error() {
        Some(error) => checks.push(fail(
            "Filesystem enforcement",
            format!("Selected backend: {backend}; this profile cannot run here: {error}"),
        )),
        None => checks.push(ok(
            "Filesystem enforcement",
            format!("Selected backend: {backend}."),
        )),
    }
    checks
}

fn validate_remote_profile(profile: &AgentProfile) -> Vec<Check> {
    let mut checks = Vec::new();
    let needs_project_environment = !profile.secrets.bindings.is_empty();
    if profile.file.is_some()
        || (profile.secrets.bindings.is_empty() && profile.personal_credentials.is_empty())
        || (needs_project_environment
            && (profile.secrets.project.is_none() || profile.secrets.environment.is_none()))
    {
        checks.push(fail(
            "Remote session profile",
            "--remote requires [secrets] to set both 'project' and 'environment' when [secrets.*] bindings are used, or supports personal-credential-only bindings without a local file. Egress-only profiles are not supported."
                .to_owned(),
        ));
    } else {
        checks.push(ok(
            "Remote session profile",
            if needs_project_environment {
                "Project/environment-backed secret bindings are compatible with --remote."
                    .to_owned()
            } else {
                "Personal credentials are compatible with --remote without project or environment."
                    .to_owned()
            },
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
    let egress_only =
        profile.secrets.bindings.is_empty() && profile.personal_credentials.is_empty();
    let personal_credentials_only =
        profile.secrets.bindings.is_empty() && !profile.personal_credentials.is_empty();
    let valid_source = if personal_credentials_only {
        profile.file.is_none()
            && matches!(
                (&profile.secrets.project, &profile.secrets.environment),
                (None, None) | (Some(_), Some(_))
            )
    } else {
        matches!(
            (
                &profile.file,
                &profile.secrets.project,
                &profile.secrets.environment
            ),
            (Some(_), None, None) | (None, Some(_), Some(_)) | (Some(_), Some(_), Some(_))
        )
    };
    if egress_only {
        if matches!(
            (
                &profile.file,
                &profile.secrets.project,
                &profile.secrets.environment
            ),
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
                "An egress-only profile must not define 'file' or a [secrets] source.".to_owned(),
            ));
        }
    } else if valid_source {
        checks.push(ok(
            "Secret source",
            if personal_credentials_only {
                "Personal credentials: remote Agent Proxy only".to_owned()
            } else {
                match (&profile.file, &profile.secrets.project) {
                    (Some(file), Some(_)) => format!("Local file '{file}' with remote fallback"),
                    (Some(file), None) => format!("Local file '{file}'"),
                    (None, Some(_)) => "Remote project and environment".to_owned(),
                    _ => unreachable!(),
                }
            },
        ));
    } else {
        checks.push(fail(
            "Secret source",
            if personal_credentials_only {
                "Personal credentials cannot use 'file' and, when [secrets] is set, require both 'project' and 'environment'."
                    .to_owned()
            } else {
                "Set 'file', both [secrets] 'project' and 'environment', or both sources together."
                    .to_owned()
            },
        ));
    }

    for (name, value) in [
        ("project", profile.secrets.project.as_deref()),
        ("environment", profile.secrets.environment.as_deref()),
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

    if profile.sandbox.image.is_some() && profile.sandbox.dockerfile.is_some() {
        checks.push(fail(
            "Sandbox image",
            "'sandbox.image' and 'sandbox.dockerfile' are mutually exclusive; set at most one."
                .to_owned(),
        ));
    }
    // The worktree's `.git` file holds a host path (`C:/...` on Windows)
    // that git inside a Linux container can't resolve.
    if profile.workspace.worktree
        && profile.sandbox.backend == crate::models::agent::SandboxBackend::Docker
        && cfg!(windows)
    {
        checks.push(fail(
            "Workspace worktree",
            "'workspace.worktree' with the Docker backend is not yet supported on Windows."
                .to_owned(),
        ));
    }
    if let Some(image) = &profile.sandbox.image {
        if image.trim().is_empty() {
            checks.push(fail(
                "Sandbox image",
                "'sandbox.image' must not be empty or whitespace.".to_owned(),
            ));
        }
    }
    if let Some(dockerfile) = &profile.sandbox.dockerfile {
        let path = Path::new(dockerfile);
        if !path.is_file() {
            checks.push(fail(
                "Sandbox Dockerfile",
                format!("File not found: {}", path.display()),
            ));
        } else {
            checks.push(ok(
                "Sandbox Dockerfile",
                format!("Readable: {}", path.display()),
            ));
        }
    }
    if let Some(memory) = &profile.sandbox.memory {
        if !valid_docker_memory_value(memory) {
            checks.push(fail(
                "Sandbox memory limit",
                format!(
                    "'{memory}' is not a valid `docker run --memory` value (expected a positive number optionally suffixed with b/k/m/g, e.g. \"2g\")."
                ),
            ));
        }
    }
    if let Some(cpus) = &profile.sandbox.cpus {
        if !valid_docker_cpus_value(cpus) {
            checks.push(fail(
                "Sandbox CPU limit",
                format!(
                    "'{cpus}' is not a valid `docker run --cpus` value (expected a positive number, e.g. \"1.5\")."
                ),
            ));
        }
    }
    for path in &profile.sandbox.isolated_paths {
        if let Err(error) = crate::handlers::run::docker_sandbox::normalize_isolated_path(path) {
            checks.push(fail("Sandbox isolated path", error));
        }
    }

    let mut bindings: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut child_envs: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut placeholders: HashMap<&str, Vec<&str>> = HashMap::new();
    for name in profile.secrets.bindings.keys() {
        if profile.personal_credentials.contains_key(name) {
            checks.push(fail(
                "Credential bindings",
                format!("Binding name '{name}' is declared in both [secrets] and [personal_credentials]."),
            ));
        }
    }
    for (target, secret) in profile
        .secrets
        .bindings
        .iter()
        .chain(profile.personal_credentials.iter())
    {
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

        if secret.hosts.is_empty() && secret.rules.is_empty() {
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
        for (index, rule) in secret.rules.iter().enumerate() {
            validate_http_rule(target, index, rule, &mut checks);
        }
        lint_http_rules(target, &secret.rules, &mut checks);
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
            .is_some_and(|template| !template.contains("{value}"))
        {
            checks.push(fail(
                format!("Binding '{target}' value_template"),
                "Must contain '{value}' so the proxy can inject the credential.".to_owned(),
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

    let mut seen_mcp_endpoints = HashSet::new();
    for (name, server) in &profile.mcp_servers {
        validate_mcp_server(name, server, &profile, &mut seen_mcp_endpoints, &mut checks);
    }
    if !profile.mcp_servers.is_empty() {
        let mut servers = profile
            .mcp_servers
            .iter()
            .map(|(name, server)| {
                format!(
                    "{name} ({} allowed, {} denied)",
                    server.allow_tools.len(),
                    server.deny_tools.len()
                )
            })
            .collect::<Vec<_>>();
        servers.sort();
        checks.push(ok(
            "MCP policy",
            format!("Configured servers: {}.", servers.join(", ")),
        ));
    }

    for (kind, paths) in [
        ("read", &profile.filesystem.deny_read),
        ("write", &profile.filesystem.deny_write),
    ] {
        let mut seen_paths = HashSet::new();
        for path in paths {
            if !valid_filesystem_path(path) {
                checks.push(fail(
                    format!("Denied filesystem {kind}"),
                    format!("'{path}' must be a non-empty path without newlines."),
                ));
            } else if let Err(error) =
                crate::handlers::run::fs_rules::parse_fs_rule(path, Path::new("/"))
            {
                checks.push(fail(format!("Denied filesystem {kind}"), error));
            } else if !seen_paths.insert(path.trim().to_owned()) {
                checks.push(warn(
                    format!("Denied filesystem {kind}"),
                    format!("Duplicate denied path '{path}'."),
                ));
            }
        }
    }

    if !uses_live_pattern_matching(profile)
        && profile
            .filesystem
            .deny_read
            .iter()
            .chain(&profile.filesystem.deny_write)
            .any(|path| crate::handlers::run::fs_rules::is_pattern_entry(path))
    {
        checks.push(warn(
            "Filesystem patterns",
            "Glob/regex entries are expanded when the run starts on this backend; files created during the run are not covered.".to_owned(),
        ));
    }

    if !checks.iter().any(|check| check.status == Status::Fail) {
        checks.push(ok(
            "Profile policy",
            format!(
                "{} binding(s), {} MCP server(s), {} egress host rule(s), {} denied host rule(s), and {} filesystem deny rule(s) are valid.",
                profile.secrets.bindings.len() + profile.personal_credentials.len(),
                profile.mcp_servers.len(),
                profile.egress_hosts.as_ref().map_or(0, Vec::len),
                profile.deny_hosts.as_ref().map_or(0, Vec::len),
                profile.filesystem.deny_read.len() + profile.filesystem.deny_write.len()
            ),
        ));
    }
    checks
}

fn validate_hook_capabilities(profile: &AgentProfile) -> Vec<Check> {
    let unsupported = profile
        .allow_hooks
        .iter()
        .filter(|hook| !matches!(hook.as_str(), "dependency_check" | "secret_scan"))
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return unsupported
            .into_iter()
            .map(|hook| {
                fail(
                    "Hook capability",
                    format!("Unsupported hook capability '{hook}'."),
                )
            })
            .collect();
    }
    let docker = profile.sandbox.backend == crate::models::agent::SandboxBackend::Docker;
    let runs_confined =
        |hook: &String| hook == "secret_scan" || (docker && hook == "dependency_check");
    if profile.allow_hooks.iter().any(runs_confined) {
        if let Some(reason) = crate::handlers::run::scan_sandbox::unavailable_reason() {
            return vec![fail("Hook capability", format!("{reason}."))];
        }
    }
    vec![ok(
        "Hook capabilities",
        if profile.allow_hooks.is_empty() {
            "No authenticated API hooks enabled.".to_owned()
        } else {
            format!("Enabled: {}.", profile.allow_hooks.join(", "))
        },
    )]
}

/// `[config_env]` names must be valid, unowned by the sandbox and unused by a
/// binding. Values that look sensitive only warn here: `agent run`
/// asks before passing them (see `env_check::confirm_config_env`).
fn validate_config_env(profile: &AgentProfile) -> Vec<Check> {
    if profile.config_env.is_empty() {
        return Vec::new();
    }
    let mut checks = Vec::new();
    let mut total_bytes = 0;
    for (name, value) in &profile.config_env {
        let value = value.value();
        let entry_bytes = env_check::config_env_entry_bytes(name, &value);
        total_bytes += entry_bytes;
        if value.contains('\0') {
            checks.push(fail(
                "Config env",
                format!("[config_env] {name} contains a NUL byte, which an environment variable can't hold."),
            ));
        }
        if entry_bytes > env_check::MAX_CONFIG_ENV_ENTRY_BYTES {
            checks.push(fail(
                "Config env",
                format!(
                    "[config_env] {name} is {entry_bytes} bytes; the limit is {} bytes per variable. Keep large content in a file in the workspace.",
                    env_check::MAX_CONFIG_ENV_ENTRY_BYTES
                ),
            ));
        }
        if !valid_environment_name(name) {
            checks.push(fail(
                "Config env",
                format!("[config_env] {name} is not a valid environment variable name."),
            ));
        } else if let Some(reason) = env_check::reserved_env_reason(name) {
            checks.push(fail("Config env", format!("[config_env] {name} {reason}.")));
        } else if let Some(binding) = env_check::conflicting_binding(profile, name) {
            checks.push(fail(
                "Config env",
                format!("[config_env] {name} is already provided by {binding}; remove it from [config_env]."),
            ));
        }
    }
    if total_bytes > env_check::MAX_CONFIG_ENV_TOTAL_BYTES {
        checks.push(fail(
            "Config env",
            format!(
                "[config_env] totals {total_bytes} bytes; the limit is {} bytes.",
                env_check::MAX_CONFIG_ENV_TOTAL_BYTES
            ),
        ));
    }
    for (name, reason) in env_check::flagged_config_env(profile) {
        checks.push(warn(
            "Config env",
            format!(
                "[config_env] {name}: {reason}. {}",
                env_check::NOT_SENSITIVE_HINT
            ),
        ));
    }
    if checks.is_empty() {
        checks.push(ok(
            "Config env",
            format!(
                "{} plaintext variable(s): {}.",
                profile.config_env.len(),
                profile
                    .config_env
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    checks
}

fn valid_filesystem_path(path: &str) -> bool {
    !path.is_empty() && path == path.trim() && !path.contains(['\r', '\n'])
}

/// Only the native macOS backend (Seatbelt) matches filesystem patterns as
/// files are accessed; every other backend expands them once at launch.
pub(crate) fn uses_live_pattern_matching(profile: &AgentProfile) -> bool {
    cfg!(target_os = "macos")
        && profile.sandbox.backend == crate::models::agent::SandboxBackend::Native
}

fn validate_http_rule(target: &str, index: usize, rule: &AgentHttpRule, checks: &mut Vec<Check>) {
    let label = format!("Secret '{target}' rule {}", index + 1);
    if rule.hosts.is_empty() {
        checks.push(fail(
            format!("{label} hosts"),
            "At least one host is required.".to_owned(),
        ));
    }
    if rule.methods.is_empty() {
        checks.push(fail(
            format!("{label} methods"),
            "At least one HTTP method is required.".to_owned(),
        ));
    }
    if rule.paths.is_empty() {
        checks.push(fail(
            format!("{label} paths"),
            "At least one path pattern is required.".to_owned(),
        ));
    }
    for host in &rule.hosts {
        if let Err(reason) = validate_host(host, false) {
            checks.push(fail(format!("{label} host"), reason));
        }
    }
    for method in &rule.methods {
        if !valid_http_method(method) {
            checks.push(fail(
                format!("{label} method"),
                format!("'{method}' is not a supported HTTP method."),
            ));
        }
    }
    for path in &rule.paths {
        if let Err(reason) = validate_path_pattern(path) {
            checks.push(fail(format!("{label} path"), reason));
        }
    }
    validate_field_conditions(&label, "body", &rule.body, checks);
    if rule.effect == AgentHttpRuleEffect::Deny && !rule.body.is_empty() {
        checks.push(warn(
            label,
            "Deny rules with body conditions are best-effort: an agent may reach the same result with a different body. Prefer allow rules with conditions.".to_owned(),
        ));
    }
}

fn validate_mcp_server(
    name: &str,
    server: &AgentMcpServer,
    profile: &AgentProfile,
    seen_endpoints: &mut HashSet<String>,
    checks: &mut Vec<Check>,
) {
    let label = format!("MCP server '{name}'");
    match reqwest::Url::parse(&server.url) {
        Ok(url)
            if matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none() =>
        {
            if url.path().is_empty() {
                checks.push(fail(
                    format!("{label} url"),
                    "The URL must include an MCP endpoint path.".to_owned(),
                ));
            }
            let endpoint = url.to_string();
            if !seen_endpoints.insert(endpoint) {
                checks.push(warn(
                    label.clone(),
                    "Duplicates another configured MCP server endpoint.".to_owned(),
                ));
            }
        }
        _ => checks.push(fail(
            format!("{label} url"),
            "Must be an absolute http:// or https:// URL with a host and no userinfo or fragment."
                .to_owned(),
        )),
    }
    if let Some(binding) = &server.binding {
        if !profile.secrets.bindings.contains_key(binding)
            && !profile.personal_credentials.contains_key(binding)
        {
            checks.push(fail(
                format!("{label} binding"),
                format!(
                    "Binding '{binding}' is not declared in [secrets] or [personal_credentials]."
                ),
            ));
        }
    }
    if let Some(header) = &server.header {
        if HeaderName::from_bytes(header.as_bytes()).is_err() {
            checks.push(fail(
                format!("{label} header"),
                format!("'{header}' is not a valid HTTP header name."),
            ));
        }
    }
    if let Some(template) = &server.value_template {
        if !template.contains("{value}") {
            checks.push(fail(
                format!("{label} value_template"),
                "Must contain '{value}' so the proxy can inject the credential.".to_owned(),
            ));
        }
    }
    validate_mcp_server_tools(&label, "allow_tools", &server.allow_tools, checks);
    validate_mcp_server_tools(&label, "deny_tools", &server.deny_tools, checks);
    for (index, rule) in server.tool_rules.iter().enumerate() {
        let rule_label = format!("{label} tool rule {}", index + 1);
        if rule.tools.is_empty() {
            checks.push(fail(
                rule_label.clone(),
                "Must name at least one tool in 'tools'.".to_owned(),
            ));
        }
        validate_mcp_server_tools(&rule_label, "tools", &rule.tools, checks);
        validate_field_conditions(&rule_label, "arguments", &rule.arguments, checks);
        if rule.effect == AgentHttpRuleEffect::Deny && !rule.arguments.is_empty() {
            checks.push(warn(
                rule_label,
                "Deny rules with argument conditions are best-effort: an agent may reach the same result with other arguments. Prefer allow rules with conditions.".to_owned(),
            ));
        }
    }
    for (rule, tool) in server.shadowed_tool_conditions() {
        checks.push(warn(
            format!("{label} tool rule {rule}"),
            shadowed_condition_message(&tool),
        ));
    }
}

/// Shared with `agent mcp` so every command explains a shadowed rule alike.
pub(crate) fn shadowed_condition_message(tool: &str) -> String {
    format!(
        "Argument conditions for '{tool}' have no effect: allow_tools or a tool rule without arguments already allows every call. Remove '{tool}' from those to enforce the conditions."
    )
}

/// Largest integer every JSON parser represents exactly as an f64.
const MAX_EXACT_NUMBER: f64 = 9_007_199_254_740_992.0;

fn validate_field_conditions(
    label: &str,
    prefix: &str,
    conditions: &BTreeMap<String, AgentFieldCondition>,
    checks: &mut Vec<Check>,
) {
    let exact = |number: f64| number.is_finite() && number.abs() <= MAX_EXACT_NUMBER;
    let scalar_kind = |scalar: &AgentScalar| match scalar {
        AgentScalar::Bool(_) => "bool",
        AgentScalar::Integer(_) | AgentScalar::Float(_) => "number",
        AgentScalar::String(_) => "string",
    };
    let scalar_exact = |scalar: &AgentScalar| match scalar {
        AgentScalar::Integer(number) => exact(*number as f64),
        AgentScalar::Float(number) => exact(*number),
        AgentScalar::Bool(_) | AgentScalar::String(_) => true,
    };
    for (path, condition) in conditions {
        let field_label = format!("{label} {prefix}.{path}");
        if path.split('.').any(|segment| segment.trim().is_empty()) {
            checks.push(fail(
                field_label.clone(),
                "Field paths are dot-separated object keys and cannot have empty segments."
                    .to_owned(),
            ));
        }
        let problem = match condition {
            AgentFieldCondition::Equals(scalar) => {
                (!scalar_exact(scalar)).then_some("Numbers must be within ±2^53.")
            }
            AgentFieldCondition::OneOf(list) if list.is_empty() => {
                Some("A list must contain at least one value.")
            }
            AgentFieldCondition::OneOf(list)
                if list
                    .iter()
                    .any(|scalar| scalar_kind(scalar) != scalar_kind(&list[0])) =>
            {
                Some("A list must contain values of one type.")
            }
            AgentFieldCondition::OneOf(list) => {
                (!list.iter().all(scalar_exact)).then_some("Numbers must be within ±2^53.")
            }
            AgentFieldCondition::Range(range) => match (range.min, range.max) {
                (None, None) => Some("A range needs 'min', 'max', or both."),
                (Some(min), Some(max)) if min > max => {
                    Some("'min' is greater than 'max', so the condition never holds.")
                }
                (min, max) if !min.into_iter().chain(max).all(exact) => {
                    Some("Numbers must be within ±2^53.")
                }
                _ => None,
            },
            AgentFieldCondition::Absent(absent) => (!absent.absent).then_some(
                "Only 'absent = true' is supported; every other condition already requires the field.",
            ),
        };
        if let Some(problem) = problem {
            checks.push(fail(field_label, problem.to_owned()));
        }
    }
}

fn validate_mcp_server_tools(label: &str, field: &str, tools: &[String], checks: &mut Vec<Check>) {
    let mut seen = HashSet::new();
    for tool in tools {
        if tool.trim().is_empty() || tool.contains(['\r', '\n']) {
            checks.push(fail(
                format!("{label} {field}"),
                "Tool names must be non-empty and cannot contain line breaks.".to_owned(),
            ));
        } else if tool != "*" && tool.contains(['*', '?']) {
            checks.push(fail(
                format!("{label} {field}"),
                format!("Invalid wildcard in tool '{tool}'; only '*' is supported."),
            ));
        } else if !seen.insert(tool) {
            checks.push(warn(
                format!("{label} {field}"),
                format!("Duplicate tool '{tool}'."),
            ));
        }
    }
}

/// Emits only conservative warnings: every reported rule relationship is
/// statically certain, while more complex wildcard overlap is left to review.
fn lint_http_rules(target: &str, rules: &[AgentHttpRule], checks: &mut Vec<Check>) {
    let mut seen = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let label = format!("Secret '{target}' rule {}", index + 1);
        if !seen.insert(rule_fingerprint(rule)) {
            checks.push(warn(
                label.clone(),
                "Duplicates an earlier HTTP action rule.".to_owned(),
            ));
        }
        if rule.paths.iter().any(|path| path == "*") {
            checks.push(warn(
                format!("{label} path"),
                "'*' matches every URL path; review this broad rule carefully.".to_owned(),
            ));
        }
        if rule.effect == AgentHttpRuleEffect::Allow
            && rules.iter().any(|deny| rule_is_fully_denied(rule, deny))
        {
            checks.push(warn(
                label.clone(),
                "Is fully shadowed by a deny rule and can never inject a credential.".to_owned(),
            ));
        }
        if rule.effect == AgentHttpRuleEffect::Allow
            && !rule.body.is_empty()
            && rules.iter().any(|other| {
                other.effect == AgentHttpRuleEffect::Allow
                    && other.body.is_empty()
                    && rule_covers(other, rule)
            })
        {
            checks.push(warn(
                label,
                "Body conditions have no effect: an allow rule without 'body' already allows these hosts, methods and paths.".to_owned(),
            ));
        }
    }
}

fn rule_fingerprint(rule: &AgentHttpRule) -> String {
    let mut hosts = rule
        .hosts
        .iter()
        .map(|host| host.trim().trim_end_matches('.').to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut methods = rule
        .methods
        .iter()
        .map(|method| method.trim().to_ascii_uppercase())
        .collect::<Vec<_>>();
    let mut paths = rule.paths.clone();
    hosts.sort();
    methods.sort();
    paths.sort();
    format!(
        "{:?}|{}|{}|{}|{:?}",
        rule.effect,
        hosts.join(","),
        methods.join(","),
        paths.join(","),
        rule.body
    )
}

/// A deny with body conditions blocks only some requests, so it never fully
/// shadows an allow.
fn rule_is_fully_denied(allow: &AgentHttpRule, deny: &AgentHttpRule) -> bool {
    deny.effect == AgentHttpRuleEffect::Deny && deny.body.is_empty() && rule_covers(deny, allow)
}

/// Whether `broad` matches every host, method and path that `narrow` does.
fn rule_covers(broad: &AgentHttpRule, narrow: &AgentHttpRule) -> bool {
    let (deny, allow) = (broad, narrow);
    allow.hosts.iter().all(|host| {
        deny.hosts
            .iter()
            .any(|denied| host_pattern_covers(denied, host))
    }) && allow.methods.iter().all(|method| {
        deny.methods
            .iter()
            .any(|denied| denied.eq_ignore_ascii_case(method))
    }) && allow.paths.iter().all(|path| {
        deny.paths
            .iter()
            .any(|denied| denied == "*" || denied == path)
    })
}

fn host_pattern_covers(denied: &str, allowed: &str) -> bool {
    let denied = denied.trim().trim_end_matches('.').to_ascii_lowercase();
    let allowed = allowed.trim().trim_end_matches('.').to_ascii_lowercase();
    if denied == allowed {
        return true;
    }
    let Some(denied_suffix) = denied.strip_prefix("*.") else {
        return false;
    };
    match allowed.strip_prefix("*.") {
        Some(allowed_suffix) => {
            allowed_suffix != denied_suffix
                && allowed_suffix.ends_with(&format!(".{denied_suffix}"))
        }
        None => allowed != denied_suffix && allowed.ends_with(&format!(".{denied_suffix}")),
    }
}

fn valid_http_method(method: &str) -> bool {
    !method.is_empty() && hyper::Method::from_bytes(method.as_bytes()).is_ok()
}

fn validate_path_pattern(path: &str) -> std::result::Result<(), String> {
    if path.is_empty()
        || path.trim() != path
        || path.chars().any(char::is_whitespace)
        || path.contains(['?', '#', '\r', '\n'])
    {
        return Err(
            "Use a non-empty URL path pattern without whitespace, query, or fragment.".to_owned(),
        );
    }
    if path != "*" && !path.starts_with('/') {
        return Err("Use '*' or a path pattern beginning with '/'.".to_owned());
    }
    if path.contains('\\') || !has_valid_percent_encoding(path) {
        return Err(
            "Path patterns cannot contain backslashes or malformed percent escapes.".to_owned(),
        );
    }
    Ok(())
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
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

/// A `docker run --memory` value: a positive number optionally suffixed
/// with a case-insensitive `b`/`k`/`m`/`g` unit (Docker's own accepted
/// format). Not an exhaustive re-implementation of Docker's own parser —
/// just enough to catch an obviously malformed value before it reaches
/// `docker run` and fails there instead.
fn valid_docker_memory_value(value: &str) -> bool {
    let value = value.trim();
    let number_part = match value.chars().last() {
        Some(suffix) if suffix.is_ascii_alphabetic() => {
            if !matches!(suffix.to_ascii_lowercase(), 'b' | 'k' | 'm' | 'g') {
                return false;
            }
            &value[..value.len() - 1]
        }
        _ => value,
    };
    number_part.parse::<f64>().is_ok_and(|number| number > 0.0)
}

/// A `docker run --cpus` value: a positive decimal number.
fn valid_docker_cpus_value(value: &str) -> bool {
    value.trim().parse::<f64>().is_ok_and(|number| number > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_hook_capabilities_in_validate_and_run() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: vec!["anything_else".to_owned()],
            config_env: Default::default(),
        };

        assert!(validate_hook_capabilities(&profile)
            .iter()
            .any(|check| check.status == Status::Fail));
        assert!(ensure_profile_is_valid_for_run(&profile)
            .unwrap_err()
            .to_string()
            .contains("Unsupported hook capability 'anything_else'"));
    }

    #[test]
    fn secret_scan_is_a_known_hook() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: vec!["dependency_check".to_owned(), "secret_scan".to_owned()],
            config_env: Default::default(),
        };

        let checks = validate_hook_capabilities(&profile);

        let confinable = crate::handlers::run::scan_sandbox::unavailable_reason().is_none();
        assert_eq!(
            checks.iter().all(|check| check.status != Status::Fail),
            confinable
        );
    }

    #[test]
    fn docker_backend_profile_gets_a_docker_runtime_check_not_native_ones() {
        // A Docker-backend profile never touches Seatbelt/systemd-run/
        // bubblewrap, so it must not be reported as unsupported on a
        // platform where only those native mechanisms are unavailable
        // (e.g. Windows) — it should get a Docker-specific check instead.
        let mut profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };
        profile.sandbox.backend = crate::models::agent::SandboxBackend::Docker;

        let checks = validate_runtime_requirements(&profile);
        assert!(checks
            .iter()
            .any(|check| check.name == "Docker sandbox backend"));
        assert!(!checks
            .iter()
            .any(|check| check.name == "Network enforcement"));
        assert!(!checks
            .iter()
            .any(|check| check.name == "Filesystem enforcement"));
    }

    fn worktree_test_profile(backend: crate::models::agent::SandboxBackend) -> AgentProfile {
        let mut profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };
        profile.workspace.worktree = true;
        profile.sandbox.backend = backend;
        profile
    }

    #[test]
    fn worktree_is_accepted_with_the_native_backend() {
        let profile = worktree_test_profile(crate::models::agent::SandboxBackend::Native);
        assert!(!validate_profile(&profile)
            .iter()
            .any(|check| check.name == "Workspace worktree"));
    }

    #[test]
    fn worktree_with_the_docker_backend_is_rejected_only_on_windows() {
        let profile = worktree_test_profile(crate::models::agent::SandboxBackend::Docker);
        let rejected = validate_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail && check.name == "Workspace worktree");
        assert_eq!(rejected, cfg!(windows));
    }

    #[test]
    fn rejects_sandbox_image_and_dockerfile_set_together() {
        let mut profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };
        profile.sandbox.image = Some("myorg/img:tag".to_owned());
        profile.sandbox.dockerfile = Some("./Cargo.toml".to_owned());

        assert!(validate_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail
                && check.name == "Sandbox image"
                && check.message.contains("mutually exclusive")));
    }

    #[test]
    fn rejects_a_missing_sandbox_dockerfile() {
        let mut profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };
        profile.sandbox.dockerfile = Some("./does-not-exist.Dockerfile".to_owned());

        assert!(validate_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail && check.name == "Sandbox Dockerfile"));
    }

    #[test]
    fn accepts_valid_docker_memory_and_cpu_values() {
        for value in ["2g", "512m", "1024k", "1", "1.5"] {
            assert!(
                valid_docker_memory_value(value),
                "expected '{value}' to be a valid memory value"
            );
        }
        for value in ["1", "1.5", "0.5", "4"] {
            assert!(
                valid_docker_cpus_value(value),
                "expected '{value}' to be a valid cpus value"
            );
        }
    }

    #[test]
    fn rejects_invalid_docker_memory_and_cpu_values() {
        for value in ["", "abc", "2x", "-1g", "0g"] {
            assert!(
                !valid_docker_memory_value(value),
                "expected '{value}' to be rejected as a memory value"
            );
        }
        for value in ["", "abc", "-1", "0"] {
            assert!(
                !valid_docker_cpus_value(value),
                "expected '{value}' to be rejected as a cpus value"
            );
        }
    }

    #[test]
    fn rejects_a_malformed_sandbox_memory_or_cpus_value() {
        let mut profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };
        profile.sandbox.memory = Some("not-a-memory-value".to_owned());
        profile.sandbox.cpus = Some("not-a-number".to_owned());
        profile.sandbox.isolated_paths = vec!["../outside".to_owned()];

        let checks = validate_profile(&profile);
        assert!(checks
            .iter()
            .any(|check| check.status == Status::Fail && check.name == "Sandbox memory limit"));
        assert!(checks
            .iter()
            .any(|check| check.status == Status::Fail && check.name == "Sandbox CPU limit"));
        assert!(checks
            .iter()
            .any(|check| check.status == Status::Fail && check.name == "Sandbox isolated path"));
    }

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
    fn rejects_malformed_http_action_rules() {
        let rule = AgentHttpRule {
            body: Default::default(),
            effect: crate::models::agent::AgentHttpRuleEffect::Allow,
            hosts: Vec::new(),
            methods: vec!["GET /".to_owned()],
            paths: vec!["repos?private=true".to_owned()],
        };
        let mut checks = Vec::new();
        validate_http_rule("TOKEN", 0, &rule, &mut checks);
        assert_eq!(
            checks
                .iter()
                .filter(|check| check.status == Status::Fail)
                .count(),
            3
        );
    }

    #[test]
    fn accepts_extension_http_methods() {
        assert!(valid_http_method("PROPFIND"));
        assert!(valid_http_method("custom-method"));
        assert!(!valid_http_method("GET /"));
    }

    #[test]
    fn lints_duplicate_broad_and_shadowed_http_rules() {
        let allow = AgentHttpRule {
            body: Default::default(),
            effect: AgentHttpRuleEffect::Allow,
            hosts: vec!["api.example.com".to_owned()],
            methods: vec!["GET".to_owned()],
            paths: vec!["/repos/*".to_owned()],
        };
        let deny = AgentHttpRule {
            body: Default::default(),
            effect: AgentHttpRuleEffect::Deny,
            hosts: vec!["api.example.com".to_owned()],
            methods: vec!["GET".to_owned()],
            paths: vec!["*".to_owned()],
        };
        let mut checks = Vec::new();
        lint_http_rules("TOKEN", &[allow, deny.clone(), deny], &mut checks);
        let messages = checks
            .iter()
            .map(|check| check.message.as_str())
            .collect::<Vec<_>>();
        assert!(messages.iter().any(|message| message.contains("shadowed")));
        assert!(messages
            .iter()
            .any(|message| message.contains("Duplicates")));
        assert!(messages
            .iter()
            .any(|message| message.contains("every URL path")));
    }

    #[test]
    fn requires_templates_to_contain_the_value_marker() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([(
                    "API_KEY".to_owned(),
                    crate::models::agent::AgentBindingProfile {
                        hosts: vec!["api.example.com".to_owned()],
                        rules: Vec::new(),
                        from: None,
                        env: None,
                        placeholder: None,
                        header: Some("x-api-key".to_owned()),
                        value_template: Some("static-value".to_owned()),
                    },
                )]),
            },
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };
        assert!(validate_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail && check.name.contains("value_template")));
    }

    #[test]
    fn rejects_legacy_secret_template_marker() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([(
                    "API_KEY".to_owned(),
                    crate::models::agent::AgentBindingProfile {
                        hosts: vec!["api.example.com".to_owned()],
                        rules: Vec::new(),
                        from: None,
                        env: None,
                        placeholder: None,
                        header: Some("Authorization".to_owned()),
                        value_template: Some("Bearer {secret}".to_owned()),
                    },
                )]),
            },
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };

        assert!(validate_profile(&profile)
            .iter()
            .any(|check| { check.status == Status::Fail && check.message.contains("{value}") }));
    }

    #[test]
    fn rejects_an_egress_only_profile_with_a_secret_source() {
        let profile = AgentProfile {
            file: Some(".env.agent".to_owned()),
            egress_hosts: Some(vec!["chatgpt.com".to_owned()]),
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };

        assert!(validate_profile(&profile).iter().any(|check| {
            check.status == Status::Fail && check.message.contains("egress-only profile")
        }));
    }

    #[test]
    fn accepts_personal_credential_only_profile_without_project_environment_or_file() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::from([(
                "LINEAR_API_KEY".to_owned(),
                crate::models::agent::AgentBindingProfile {
                    hosts: vec!["mcp.linear.app".to_owned()],
                    rules: Vec::new(),
                    from: None,
                    env: None,
                    placeholder: None,
                    header: None,
                    value_template: None,
                },
            )]),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };

        assert!(!validate_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail));
        assert!(!validate_remote_profile(&profile)
            .iter()
            .any(|check| check.status == Status::Fail));
    }

    #[test]
    fn run_validation_rejects_duplicate_remote_placeholders() {
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([
                    (
                        "FIRST_KEY".to_owned(),
                        crate::models::agent::AgentBindingProfile {
                            hosts: vec!["first.example.com".to_owned()],
                            rules: Vec::new(),
                            from: None,
                            env: None,
                            placeholder: Some("shared-placeholder".to_owned()),
                            header: None,
                            value_template: None,
                        },
                    ),
                    (
                        "SECOND_KEY".to_owned(),
                        crate::models::agent::AgentBindingProfile {
                            hosts: vec!["second.example.com".to_owned()],
                            rules: Vec::new(),
                            from: None,
                            env: None,
                            placeholder: Some("shared-placeholder".to_owned()),
                            header: None,
                            value_template: None,
                        },
                    ),
                ]),
            },
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };

        let error = ensure_profile_is_valid_for_run(&profile).unwrap_err();
        assert!(error
            .to_string()
            .contains("Placeholder 'shared-placeholder'"));
    }

    #[test]
    fn run_validation_rejects_binding_name_shared_by_secret_and_credential() {
        let binding = crate::models::agent::AgentBindingProfile {
            hosts: vec!["api.example.com".to_owned()],
            rules: Vec::new(),
            from: None,
            env: None,
            placeholder: None,
            header: None,
            value_template: None,
        };
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            allow_network_listeners: false,
            deny_hosts: None,
            filesystem: Default::default(),
            sandbox: Default::default(),
            workspace: Default::default(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([("API_KEY".to_owned(), binding.clone())]),
            },
            personal_credentials: HashMap::from([("API_KEY".to_owned(), binding)]),
            policy_tests: Vec::new(),
            allow_hooks: Vec::new(),
            config_env: Default::default(),
        };

        let error = ensure_profile_is_valid_for_run(&profile).unwrap_err();
        assert!(error
            .to_string()
            .contains("declared in both [secrets] and [personal_credentials]"));
    }

    fn filesystem_test_profile(deny_read: &[&str]) -> AgentProfile {
        let mut profile = worktree_test_profile(crate::models::agent::SandboxBackend::Native);
        profile.workspace.worktree = false;
        profile.filesystem.deny_read = deny_read.iter().map(|path| (*path).to_owned()).collect();
        profile
    }

    fn filesystem_checks(profile: &AgentProfile) -> Vec<Check> {
        validate_profile(profile)
            .into_iter()
            .filter(|check| {
                check.name.starts_with("Denied filesystem") || check.name == "Filesystem patterns"
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn accepts_glob_and_regex_filesystem_entries() {
        let profile = filesystem_test_profile(&["**/.env*", "re:^~/.*\\.pem$", "~/notes/[old]"]);
        assert!(!filesystem_checks(&profile)
            .iter()
            .any(|check| check.status == Status::Fail));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_invalid_filesystem_patterns() {
        for (entry, message) in [
            ("re:.*\\.pem$", "must start with"),
            ("re:^/a/\\d+", "Seatbelt"),
            ("../*.env", "'..'"),
        ] {
            let checks = filesystem_checks(&filesystem_test_profile(&[entry]));
            assert!(
                checks
                    .iter()
                    .any(|check| check.status == Status::Fail && check.message.contains(message)),
                "{entry}: {:?}",
                checks
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn warns_that_patterns_are_a_launch_snapshot_outside_seatbelt() {
        let mut profile = filesystem_test_profile(&["**/.env*"]);
        profile.sandbox.backend = crate::models::agent::SandboxBackend::Docker;
        assert!(filesystem_checks(&profile)
            .iter()
            .any(|check| check.name == "Filesystem patterns" && check.status == Status::Warn));

        let plain = filesystem_test_profile(&["~/.ssh"]);
        assert!(!filesystem_checks(&plain)
            .iter()
            .any(|check| check.name == "Filesystem patterns"));
    }

    #[test]
    fn mcp_tool_rules_validate_argument_conditions() {
        let profile: AgentProfile = toml::from_str(
            r#"
[mcp_servers.mail]
url = "https://mcp.mail.example/mcp"

[[mcp_servers.mail.tool_rules]]
effect = "allow"
tools = ["send_message"]
arguments = { mode = "draft", to = ["support@acme.com"], amount = { min = 1, max = 5000 }, id = { absent = true } }

[[mcp_servers.mail.tool_rules]]
effect = "allow"
tools = []
arguments = { "a..b" = 1, kinds = ["a", 1], empty = [], range = {}, backwards = { min = 5, max = 1 }, huge = 1e300, present = { absent = false } }

[[mcp_servers.mail.tool_rules]]
effect = "deny"
tools = ["send_message"]
arguments = { priority = "urgent" }
"#,
        )
        .unwrap();
        let checks = validate_profile(&profile);
        let messages = |status: Status, label: &str| {
            checks
                .iter()
                .filter(|check| check.status == status && check.name.starts_with(label))
                .map(|check| check.message.as_str())
                .collect::<Vec<_>>()
        };
        assert!(messages(Status::Fail, "MCP server 'mail' tool rule 1").is_empty());
        let failures = messages(Status::Fail, "MCP server 'mail' tool rule 2");
        for expected in [
            "Must name at least one tool",
            "cannot have empty segments",
            "values of one type",
            "at least one value",
            "needs 'min', 'max', or both",
            "never holds",
            "within ±2^53",
            "Only 'absent = true'",
        ] {
            assert!(
                failures.iter().any(|message| message.contains(expected)),
                "missing '{expected}' in {failures:?}"
            );
        }
        assert!(messages(Status::Warn, "MCP server 'mail' tool rule 3")
            .iter()
            .any(|message| message.contains("best-effort")));
        assert!(
            !checks
                .iter()
                .any(|check| check.message.contains("have no effect")),
            "nothing allows send_message without conditions"
        );
    }

    #[test]
    fn http_rules_validate_body_conditions() {
        let profile: AgentProfile = toml::from_str(
            r#"
[secrets.PAYMENTS_KEY]

[[secrets.PAYMENTS_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]
body = { currency = "usd", amount = { min = 1, max = 5000 }, "destination.type" = "bank" }

[[secrets.PAYMENTS_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/refunds"]
body = { "a..b" = 1, empty = [], backwards = { min = 5, max = 1 } }

[[secrets.PAYMENTS_KEY.rules]]
effect = "deny"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]
body = { "destination.type" = "card" }

[secrets.MAIL_KEY]

[[secrets.MAIL_KEY.rules]]
effect = "allow"
hosts = ["api.mail.example"]
methods = ["POST"]
paths = ["/v1/messages"]

[[secrets.MAIL_KEY.rules]]
effect = "allow"
hosts = ["api.mail.example"]
methods = ["POST"]
paths = ["/v1/messages"]
body = { send = false }
"#,
        )
        .unwrap();
        let checks = validate_profile(&profile);
        let messages = |status: Status, label: &str| {
            checks
                .iter()
                .filter(|check| check.status == status && check.name.starts_with(label))
                .map(|check| check.message.as_str())
                .collect::<Vec<_>>()
        };
        assert!(messages(Status::Fail, "Secret 'PAYMENTS_KEY' rule 1").is_empty());
        let failures = messages(Status::Fail, "Secret 'PAYMENTS_KEY' rule 2");
        for expected in [
            "cannot have empty segments",
            "at least one value",
            "never holds",
        ] {
            assert!(
                failures.iter().any(|message| message.contains(expected)),
                "missing '{expected}' in {failures:?}"
            );
        }
        assert!(messages(Status::Warn, "Secret 'PAYMENTS_KEY' rule 3")
            .iter()
            .any(|message| message.contains("best-effort")));
        assert!(
            !messages(Status::Warn, "Secret 'PAYMENTS_KEY' rule 1")
                .iter()
                .any(|message| message.contains("shadowed")),
            "a deny with body conditions never fully shadows an allow"
        );
        assert!(messages(Status::Warn, "Secret 'MAIL_KEY' rule 2")
            .iter()
            .any(|message| message.contains("Body conditions have no effect")));
    }

    #[test]
    fn http_rules_reject_unknown_fields() {
        let error = toml::from_str::<AgentProfile>(
            r#"
[secrets.MAIL_KEY]

[[secrets.MAIL_KEY.rules]]
effect = "allow"
hosts = ["api.mail.example"]
methods = ["POST"]
paths = ["/v1/messages"]
bodyy = { send = false }
"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("bodyy"), "{error}");
    }

    #[test]
    fn mcp_tool_rules_warn_when_allow_tools_makes_conditions_moot() {
        let profile: AgentProfile = toml::from_str(
            r#"
[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
allow_tools = ["*"]

[[mcp_servers.linear.tool_rules]]
effect = "allow"
tools = ["save_issue"]
arguments = { team = ["Engineering"] }
"#,
        )
        .unwrap();
        let checks = validate_profile(&profile);
        assert!(checks.iter().any(|check| check.status == Status::Warn
            && check.name == "MCP server 'linear' tool rule 1"
            && check.message.contains("'save_issue' have no effect")));
    }

    fn config_env_profile(toml_source: &str) -> AgentProfile {
        toml::from_str(toml_source).unwrap()
    }

    fn config_env_statuses(profile: &AgentProfile) -> Vec<(Status, String)> {
        validate_config_env(profile)
            .into_iter()
            .map(|check| (check.status, check.message))
            .collect()
    }

    #[test]
    fn config_env_rejects_reserved_invalid_and_bound_names() {
        let profile = config_env_profile(
            "[config_env]\nHTTPS_PROXY = \"http://evil\"\n\"1BAD\" = \"x\"\nGITHUB_TOKEN = \"x\"\n\n[secrets]\nproject = \"p\"\nenvironment = \"e\"\n[secrets.GITHUB_TOKEN]\n",
        );
        let checks = config_env_statuses(&profile);
        let failures = checks
            .iter()
            .filter(|(status, _)| *status == Status::Fail)
            .map(|(_, message)| message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 3, "{checks:?}");
        assert!(failures
            .iter()
            .any(|m| m.contains("HTTPS_PROXY is set by the Stashbase sandbox")));
        assert!(failures.iter().any(|m| m.contains("1BAD is not a valid")));
        assert!(failures
            .iter()
            .any(|m| m.contains("GITHUB_TOKEN is already provided by [secrets.GITHUB_TOKEN]")));
        assert!(ensure_profile_is_valid_for_run(&profile)
            .unwrap_err()
            .to_string()
            .contains("is already provided by"));
    }

    #[test]
    fn config_env_warns_on_sensitive_looking_values_without_printing_them() {
        let profile = config_env_profile(
            "[config_env]\nNODE_ENV = \"development\"\nUPLOAD_URL = \"https://u:hunter2@example.com\"\nBUILD = { value = \"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b\", not_sensitive = true }\n",
        );
        let checks = config_env_statuses(&profile);
        assert_eq!(checks.len(), 1, "{checks:?}");
        assert_eq!(checks[0].0, Status::Warn);
        assert!(checks[0].1.contains("UPLOAD_URL"));
        assert!(!checks[0].1.contains("hunter2"));
    }

    #[test]
    fn config_env_reports_plain_variables_ok() {
        let profile = config_env_profile("[config_env]\nNODE_ENV = \"development\"\n");
        let checks = config_env_statuses(&profile);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].0, Status::Ok);
        assert!(config_env_statuses(&config_env_profile("")).is_empty());
    }

    fn config_env_failures(profile: &AgentProfile) -> Vec<String> {
        config_env_statuses(profile)
            .into_iter()
            .filter(|(status, _)| *status == Status::Fail)
            .map(|(_, message)| message)
            .collect()
    }

    #[test]
    fn config_env_rejects_nul_bytes() {
        let profile = config_env_profile("[config_env]\nGREETING = \"hi\\u0000there\"\n");
        let failures = config_env_failures(&profile);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("GREETING contains a NUL byte"));
        assert!(!failures[0].contains("there"));
    }

    #[test]
    fn config_env_limits_each_value_size() {
        let at_limit = "a".repeat(env_check::MAX_CONFIG_ENV_ENTRY_BYTES - "BLOB=".len());
        let mut profile = config_env_profile("");
        profile.config_env.insert(
            "BLOB".to_owned(),
            crate::models::agent::AgentEnvValue::Plain(
                crate::models::agent::AgentEnvScalar::String(at_limit.clone()),
            ),
        );
        assert!(config_env_failures(&profile).is_empty());

        profile.config_env.insert(
            "BLOB".to_owned(),
            crate::models::agent::AgentEnvValue::Plain(
                crate::models::agent::AgentEnvScalar::String(at_limit + "a"),
            ),
        );
        let failures = config_env_failures(&profile);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("BLOB is 65537 bytes; the limit is 65536 bytes per variable"));
    }

    #[test]
    fn config_env_limits_total_size() {
        let value = "a".repeat(60 * 1024);
        let mut profile = config_env_profile("");
        for index in 0..5 {
            profile.config_env.insert(
                format!("PART_{index}"),
                crate::models::agent::AgentEnvValue::Plain(
                    crate::models::agent::AgentEnvScalar::String(value.clone()),
                ),
            );
        }
        let failures = config_env_failures(&profile);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("the limit is 262144 bytes"));
    }
}
