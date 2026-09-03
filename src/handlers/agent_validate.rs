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
    models::{
        agent::{AgentHttpRule, AgentHttpRuleEffect, AgentMcpRule, AgentMcpServer, AgentProfile},
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
    let backend = crate::handlers::run::subprocess::filesystem_backend_for_policy(
        &profile.filesystem.deny_read,
        &profile.filesystem.deny_write,
    );
    if profile.filesystem.deny_read.is_empty() && profile.filesystem.deny_write.is_empty() {
        return vec![ok(
            "Filesystem enforcement",
            format!("Selected backend: {backend}."),
        )];
    }

    match crate::handlers::run::subprocess::filesystem_enforcement_error() {
        Some(error) => vec![fail(
            "Filesystem enforcement",
            format!("Selected backend: {backend}; this profile cannot run here: {error}"),
        )],
        None => vec![ok(
            "Filesystem enforcement",
            format!("Selected backend: {backend}."),
        )],
    }
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

    for (index, rule) in profile.mcp_rules.iter().enumerate() {
        validate_mcp_rule(index, rule, &mut checks);
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
                    if server.allow_tools.is_empty() {
                        "*".to_owned()
                    } else {
                        server.allow_tools.len().to_string()
                    },
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
                    format!(
                        "'{path}' must be a non-empty path without newlines or glob characters."
                    ),
                ));
            } else if !seen_paths.insert(path.trim().to_owned()) {
                checks.push(warn(
                    format!("Denied filesystem {kind}"),
                    format!("Duplicate denied path '{path}'."),
                ));
            }
        }
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

fn valid_filesystem_path(path: &str) -> bool {
    !path.is_empty()
        && path == path.trim()
        && !path.contains(['\r', '\n'])
        && !path.contains(['*', '?'])
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
}

fn validate_mcp_rule(index: usize, rule: &AgentMcpRule, checks: &mut Vec<Check>) {
    let label = format!("MCP rule {}", index + 1);
    if rule.hosts.is_empty() {
        checks.push(fail(
            format!("{label} hosts"),
            "At least one host is required.".to_owned(),
        ));
    }
    if rule.paths.is_empty() {
        checks.push(fail(
            format!("{label} paths"),
            "At least one path is required.".to_owned(),
        ));
    }
    if rule.tools.is_empty() {
        checks.push(fail(
            format!("{label} tools"),
            "At least one tool is required.".to_owned(),
        ));
    }
    for host in &rule.hosts {
        if let Err(reason) = validate_host(host, false) {
            checks.push(fail(format!("{label} host"), reason));
        }
    }
    for path in &rule.paths {
        if let Err(reason) = validate_path_pattern(path) {
            checks.push(fail(format!("{label} path"), reason));
        }
    }
    let mut seen = HashSet::new();
    for tool in &rule.tools {
        if tool.trim().is_empty() || tool.contains(['\r', '\n']) {
            checks.push(fail(
                format!("{label} tool"),
                "Tool names must be non-empty and cannot contain line breaks.".to_owned(),
            ));
        } else if !seen.insert(tool) {
            checks.push(warn(
                format!("{label} tool"),
                format!("Duplicate tool '{tool}'."),
            ));
        }
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
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() => {
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
            "Must be an absolute http:// or https:// URL with a host.".to_owned(),
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
    validate_mcp_server_tools(&label, "allow_tools", &server.allow_tools, checks);
    validate_mcp_server_tools(&label, "deny_tools", &server.deny_tools, checks);
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
                label,
                "Is fully shadowed by a deny rule and can never inject a credential.".to_owned(),
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
        "{:?}|{}|{}|{}",
        rule.effect,
        hosts.join(","),
        methods.join(","),
        paths.join(",")
    )
}

fn rule_is_fully_denied(allow: &AgentHttpRule, deny: &AgentHttpRule) -> bool {
    deny.effect == AgentHttpRuleEffect::Deny
        && allow.hosts.iter().all(|host| {
            deny.hosts
                .iter()
                .any(|denied| host_pattern_covers(denied, host))
        })
        && allow.methods.iter().all(|method| {
            deny.methods
                .iter()
                .any(|denied| denied.eq_ignore_ascii_case(method))
        })
        && allow.paths.iter().all(|path| {
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
    fn rejects_malformed_http_action_rules() {
        let rule = AgentHttpRule {
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
            effect: AgentHttpRuleEffect::Allow,
            hosts: vec!["api.example.com".to_owned()],
            methods: vec!["GET".to_owned()],
            paths: vec!["/repos/*".to_owned()],
        };
        let deny = AgentHttpRule {
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
            deny_hosts: None,
            filesystem: Default::default(),
            mcp_rules: Vec::new(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([(
                    "API_KEY".to_owned(),
                    crate::models::agent::AgentBindingProfile {
                        hosts: vec!["api.example.com".to_owned()],
                        rules: Vec::new(),
                        mcp_rules: Vec::new(),
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
            deny_hosts: None,
            filesystem: Default::default(),
            mcp_rules: Vec::new(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([(
                    "API_KEY".to_owned(),
                    crate::models::agent::AgentBindingProfile {
                        hosts: vec!["api.example.com".to_owned()],
                        rules: Vec::new(),
                        mcp_rules: Vec::new(),
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
            deny_hosts: None,
            filesystem: Default::default(),
            mcp_rules: Vec::new(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::new(),
            policy_tests: Vec::new(),
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
            deny_hosts: None,
            filesystem: Default::default(),
            mcp_rules: Vec::new(),
            mcp_servers: HashMap::new(),
            secrets: HashMap::new().into(),
            personal_credentials: HashMap::from([(
                "LINEAR_API_KEY".to_owned(),
                crate::models::agent::AgentBindingProfile {
                    hosts: vec!["mcp.linear.app".to_owned()],
                    rules: Vec::new(),
                    mcp_rules: Vec::new(),
                    from: None,
                    env: None,
                    placeholder: None,
                    header: None,
                    value_template: None,
                },
            )]),
            policy_tests: Vec::new(),
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
            deny_hosts: None,
            filesystem: Default::default(),
            mcp_rules: Vec::new(),
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
                            mcp_rules: Vec::new(),
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
                            mcp_rules: Vec::new(),
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
            mcp_rules: Vec::new(),
            from: None,
            env: None,
            placeholder: None,
            header: None,
            value_template: None,
        };
        let profile = AgentProfile {
            file: None,
            egress_hosts: None,
            deny_hosts: None,
            filesystem: Default::default(),
            mcp_rules: Vec::new(),
            mcp_servers: HashMap::new(),
            secrets: crate::models::agent::AgentSecretsProfile {
                project: Some("project".to_owned()),
                environment: Some("development".to_owned()),
                bindings: HashMap::from([("API_KEY".to_owned(), binding.clone())]),
            },
            personal_credentials: HashMap::from([("API_KEY".to_owned(), binding)]),
            policy_tests: Vec::new(),
        };

        let error = ensure_profile_is_valid_for_run(&profile).unwrap_err();
        assert!(error
            .to_string()
            .contains("declared in both [secrets] and [personal_credentials]"));
    }
}
