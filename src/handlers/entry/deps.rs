use anyhow::{bail, Context, Result};
use std::{collections::BTreeSet, fs, io::Read, path::Path};

use crate::{
    api::dependencies::check_batch,
    cmd::deps::{
        AgentDepsSubcommand, AgentHookTarget, AgentHooksCommand, AgentHooksSubcommand, HookAgent,
    },
    models::dependencies::{DependencyCheckRequest, DependencyDecision},
};

pub async fn handle_agent_hooks_commands(cmd: AgentHooksCommand, api_key: String) -> Result<()> {
    match cmd.subcommand {
        Some(AgentHooksSubcommand::Deps(args)) => match args.subcommand {
            AgentDepsSubcommand::Install(args) => in_scope(&args, HookAction::Install),
            AgentDepsSubcommand::Check(args) => in_scope(&args, HookAction::Check),
            AgentDepsSubcommand::Uninstall(args) => in_scope(&args, HookAction::Uninstall),
        },
        None => return handle_hook(api_key).await,
    }
}

/// Which agent configuration a hook command reads or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HookScope {
    /// This repository's config, which a Docker run also sees.
    Project,
    /// This machine's home directory config.
    Global,
    /// The Docker sandbox's home volume, shared by every Docker-backend run.
    /// The config files are staged in a host directory while they change.
    Docker,
}

impl HookScope {
    fn label(self) -> &'static str {
        match self {
            HookScope::Project => "project",
            HookScope::Global => "global",
            HookScope::Docker => "Docker sandbox",
        }
    }
}

#[derive(Clone, Copy)]
enum HookAction {
    Install,
    Check,
    Uninstall,
}

/// Agent config files kept in the Docker sandbox's home, relative to it.
const DOCKER_HOOK_FILES: &[&str] = &[
    ".claude/settings.json",
    ".codex/hooks.json",
    ".codex/config.toml",
    ".cursor/hooks.json",
];

fn in_scope(target: &AgentHookTarget, action: HookAction) -> Result<()> {
    if target.docker {
        let write_back = !matches!(action, HookAction::Check);
        return crate::handlers::run::docker_sandbox::with_persistent_home_files(
            DOCKER_HOOK_FILES,
            write_back,
            |root| apply(target.agent, action, root, HookScope::Docker),
        );
    }
    let (scope, root) = if target.global {
        let home = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .context("Could not determine the home directory for global hooks.")?;
        (HookScope::Global, home)
    } else {
        let root = git2::Repository::discover(".")
            .context("Hook commands without --global or --docker must run inside a git repository.")?
            .workdir()
            .map(Path::to_path_buf)
            .context("Git repository has no working directory.")?;
        (HookScope::Project, root)
    };
    apply(target.agent, action, &root, scope)
}

fn apply(agent: HookAgent, action: HookAction, root: &Path, scope: HookScope) -> Result<()> {
    match (action, agent) {
        (HookAction::Install, HookAgent::Claude) => install_claude_hook(root, scope),
        (HookAction::Install, HookAgent::Codex) => install_codex_hook(root, scope),
        (HookAction::Install, HookAgent::Cursor) => install_cursor_hook(root, scope),
        (HookAction::Uninstall, HookAgent::Claude) => uninstall_claude_hook(root, scope),
        (HookAction::Uninstall, HookAgent::Codex) => uninstall_codex_hook(root, scope),
        (HookAction::Uninstall, HookAgent::Cursor) => uninstall_cursor_hook(root, scope),
        (HookAction::Check, agent) => check_hook(agent, root, scope),
    }
}

/// Codex keeps its global config in `CODEX_HOME` when that is set. Inside
/// the Docker sandbox it is not, so the volume's `.codex` is used.
fn codex_directory(root: &Path, scope: HookScope) -> std::path::PathBuf {
    if scope == HookScope::Global {
        if let Some(home) = std::env::var_os("CODEX_HOME") {
            return std::path::PathBuf::from(home);
        }
    }
    root.join(".codex")
}

/// `path` as the agent sees it: for the Docker scope, inside the sandbox's
/// home rather than the host staging directory.
fn shown(path: &Path, root: &Path, scope: HookScope) -> String {
    match (scope, path.strip_prefix(root)) {
        (HookScope::Docker, Ok(relative)) => Path::new("/home/agent")
            .join(relative)
            .display()
            .to_string(),
        _ => path.display().to_string(),
    }
}

fn check_hook(agent: HookAgent, root: &Path, scope: HookScope) -> Result<()> {
    let (path, installed) = match agent {
        HookAgent::Claude => {
            let path = root.join(".claude/settings.json");
            let installed = has_dependency_hook(&path)?;
            (path, installed)
        }
        HookAgent::Codex => {
            let path = codex_directory(root, scope).join("hooks.json");
            let installed = has_dependency_hook(&path)?;
            (path, installed)
        }
        HookAgent::Cursor => {
            let path = root.join(".cursor/hooks.json");
            let installed = has_cursor_dependency_hook(&path)?;
            (path, installed)
        }
    };
    if installed {
        println!("Dependency hook installed in {}", shown(&path, root, scope));
        Ok(())
    } else {
        bail!("No dependency hook found in {}", shown(&path, root, scope));
    }
}

async fn handle_hook(api_key: String) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    if input.trim().is_empty() {
        return Ok(());
    }

    let input: serde_json::Value =
        serde_json::from_str(&input).context("Agent hook input was not valid JSON.")?;
    let event = input
        .get("hook_event_name")
        .or_else(|| input.get("event_name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("PostToolUse");
    if std::env::var(crate::api::dependencies::HOOK_MODE_ENV).as_deref() == Ok("disabled") {
        if event == "beforeShellExecution" {
            println!("{}", serde_json::json!({"permission": "allow"}));
        }
        return Ok(());
    }
    if matches!(event, "PreToolUse" | "preToolUse" | "beforeShellExecution") {
        let Some(dependencies) = parse_preinstall_dependencies(&input)? else {
            return Ok(());
        };
        if dependencies.is_empty() {
            if event == "beforeShellExecution" {
                println!("{}", serde_json::json!({"permission": "allow"}));
            }
            return Ok(());
        }
        return check_pre_install(api_key, dependencies, event).await;
    }
    Ok(())
}

async fn check_pre_install(
    api_key: String,
    dependencies: Vec<DependencyCheckRequest>,
    event: &str,
) -> Result<()> {
    let response = match check_batch(api_key, dependencies).await {
        Ok(response) => response,
        Err(error) => {
            print_pre_hook_denial(event, &format!("Dependency check failed: {error}"));
            return Ok(());
        }
    };
    print_pre_hook_response(event, &response);
    Ok(())
}

fn print_pre_hook_response(
    event: &str,
    response: &crate::models::dependencies::DependencyCheckBatchResponse,
) {
    let blocked = response.decision == DependencyDecision::Block;
    let reason = response
        .dependencies
        .iter()
        .flat_map(|dependency| dependency.reasons.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if event == "beforeShellExecution" {
        let message = if reason.is_empty() {
            "Dependency check blocked this install.".to_owned()
        } else {
            reason.clone()
        };
        println!(
            "{}",
            serde_json::json!({
                "permission": if blocked { "deny" } else { "allow" },
                "user_message": if blocked { message } else { format!("Stashbase dependency warning: {message}") }
            })
        );
    } else if blocked {
        print_pre_hook_denial(event, &reason);
    } else if response.decision == DependencyDecision::Warn {
        println!(
            "{}",
            serde_json::json!({
                "systemMessage": format!("Stashbase dependency warning: {reason}"),
                "hookSpecificOutput": {"hookEventName": event}
            })
        );
    } else {
        println!(
            "{}",
            serde_json::json!({"hookSpecificOutput": {"hookEventName": event}})
        );
    }
}

fn print_pre_hook_denial(event: &str, reason: &str) {
    if event == "beforeShellExecution" {
        println!(
            "{}",
            serde_json::json!({"permission": "deny", "user_message": reason})
        );
        return;
    }
    println!(
        "{}",
        serde_json::json!({
            "decision": "block",
            "reason": reason,
            "hookSpecificOutput": {
                "hookEventName": event,
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }
        })
    );
}

fn parse_preinstall_dependencies(
    input: &serde_json::Value,
) -> Result<Option<Vec<DependencyCheckRequest>>> {
    let Some(command) = [
        input.pointer("/tool_input/command"),
        input.get("command"),
        input.pointer("/input/command"),
    ]
    .into_iter()
    .flatten()
    .find_map(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let mut dependencies = Vec::new();
    let mut found_install = false;
    for segment in command.split(['&', '|', ';']) {
        let words = segment
            .split_whitespace()
            .map(|word| word.trim_matches(['\'', '"']))
            .collect::<Vec<_>>();
        for index in 0..words.len().saturating_sub(1) {
            let is_manager = matches!(words[index], "npm" | "bun" | "pnpm" | "yarn");
            let is_install = matches!(
                (words[index], words[index + 1]),
                ("npm", "install" | "i" | "ci")
                    | ("bun", "add" | "install")
                    | ("pnpm", "add" | "install" | "i")
                    | ("yarn", "add" | "install")
            );
            if !is_manager || !is_install {
                continue;
            }
            found_install = true;
            for spec in words[index + 2..]
                .iter()
                .copied()
                .filter(|word| !word.starts_with('-'))
            {
                dependencies.push(parse_package_spec(spec)?);
            }
            break;
        }
    }
    if found_install && dependencies.is_empty() {
        dependencies = root_dependency_requests(input)?;
    }
    Ok(found_install.then_some(dependencies))
}

fn root_dependency_requests(input: &serde_json::Value) -> Result<Vec<DependencyCheckRequest>> {
    let root = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .context("Could not determine the project directory for dependency scanning.")?;
    let manifest = root.join("package.json");
    if !manifest.exists() {
        return Ok(Vec::new());
    }
    let manifest = read_json(&manifest)?;
    let names = [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ]
    .into_iter()
    .filter_map(|field| manifest.get(field).and_then(serde_json::Value::as_object))
    .flat_map(|dependencies| dependencies.keys().cloned())
    .collect::<BTreeSet<_>>();
    let versions = root_lockfile_versions(&root, &names);
    names
        .into_iter()
        .map(|name| match versions.get(&name) {
            Some(version) => DependencyCheckRequest::new(&name, version)
                .map_err(|error| anyhow::anyhow!("Cannot check dependency '{name}': {error}")),
            None => DependencyCheckRequest::latest(name)
                .map_err(|error| anyhow::anyhow!("Cannot check dependency: {error}")),
        })
        .collect()
}

fn root_lockfile_versions(
    root: &Path,
    names: &BTreeSet<String>,
) -> std::collections::BTreeMap<String, String> {
    let mut versions = std::collections::BTreeMap::new();
    if let Ok(lockfile) = read_json(&root.join("package-lock.json")) {
        if let Some(packages) = lockfile
            .get("packages")
            .and_then(serde_json::Value::as_object)
        {
            for name in names {
                if let Some(version) = packages
                    .get(&format!("node_modules/{name}"))
                    .and_then(|package| package.get("version"))
                    .and_then(serde_json::Value::as_str)
                {
                    versions.insert(name.clone(), version.to_owned());
                }
            }
        } else if let Some(dependencies) = lockfile
            .get("dependencies")
            .and_then(serde_json::Value::as_object)
        {
            for name in names {
                if let Some(version) = dependencies
                    .get(name)
                    .and_then(|dependency| dependency.get("version"))
                    .and_then(serde_json::Value::as_str)
                {
                    versions.insert(name.clone(), version.to_owned());
                }
            }
        }
    }
    if let Some(lockfile) = fs::read_to_string(root.join("pnpm-lock.yaml"))
        .ok()
        .and_then(|contents| serde_yaml::from_str::<serde_yaml::Value>(&contents).ok())
    {
        let importer = lockfile
            .as_mapping()
            .and_then(|root| root.get(serde_yaml::Value::String("importers".to_owned())))
            .and_then(serde_yaml::Value::as_mapping)
            .and_then(|importers| importers.get(serde_yaml::Value::String(".".to_owned())));
        if let Some(importer) = importer {
            for field in ["dependencies", "devDependencies", "optionalDependencies"] {
                if let Some(entries) = importer.get(field).and_then(serde_yaml::Value::as_mapping) {
                    for name in names {
                        if let Some(version) = entries
                            .get(serde_yaml::Value::String(name.clone()))
                            .and_then(|entry| entry.get("version").or(Some(entry)))
                            .and_then(serde_yaml::Value::as_str)
                            .map(|version| version.split('(').next().unwrap_or(version))
                        {
                            versions.insert(name.clone(), version.to_owned());
                        }
                    }
                }
            }
        }
    }
    versions
}

fn parse_package_spec(spec: &str) -> Result<DependencyCheckRequest> {
    if spec.is_empty() || spec.contains(':') || spec.starts_with(['.', '/']) {
        bail!("Pre-install checks support only npm package names and versions.");
    }
    let (name, version) = match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() => (name, Some(version)),
        _ => (spec, None),
    };
    match version {
        Some(version) => DependencyCheckRequest::new(name, version)
            .map_err(|error| anyhow::anyhow!("Cannot check dependency '{spec}': {error}")),
        None => DependencyCheckRequest::latest(name)
            .map_err(|error| anyhow::anyhow!("Cannot check dependency '{spec}': {error}")),
    }
}

fn uninstall_claude_hook(root: &Path, scope: HookScope) -> Result<()> {
    let path = root.join(".claude/settings.json");
    if !path.exists() {
        println!("No Claude dependency hook found in {}", shown(&path, root, scope));
        return Ok(());
    }
    let mut settings = read_json(&path).context("Failed to read .claude/settings.json.")?;
    if !remove_tool_hook(&mut settings) {
        println!("No Claude dependency hook found in {}", shown(&path, root, scope));
        return Ok(());
    }
    write_json(&path, &settings)?;
    println!(
        "Removed {} Claude dependency hook from {}",
        scope.label(),
        shown(&path, root, scope)
    );
    Ok(())
}

fn uninstall_codex_hook(root: &Path, scope: HookScope) -> Result<()> {
    let directory = codex_directory(root, scope);
    let path = directory.join("hooks.json");
    if !path.exists() {
        println!("No Codex dependency hook found in {}", shown(&path, root, scope));
        return Ok(());
    }
    let mut hooks = read_json(&path).context("Failed to read .codex/hooks.json.")?;
    if !remove_tool_hook(&mut hooks) {
        println!("No Codex dependency hook found in {}", shown(&path, root, scope));
        return Ok(());
    }
    write_json(&path, &hooks)?;
    println!(
        "Removed {} Codex dependency hook from {}",
        scope.label(),
        shown(&path, root, scope)
    );
    Ok(())
}

fn install_cursor_hook(root: &Path, scope: HookScope) -> Result<()> {
    let directory = root.join(".cursor");
    let other_path = match scope {
        HookScope::Global => git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
            .map(|root| root.join(".cursor/hooks.json")),
        HookScope::Project => {
            directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".cursor/hooks.json"))
        }
        HookScope::Docker => None,
    };
    let path = directory.join("hooks.json");
    if let Some(other_path) = other_path.as_deref().filter(|other| *other != path) {
        if has_cursor_dependency_hook(other_path)? {
            println!(
                "Dependency hook already exists in the other Cursor configuration scope; remove it before installing in {}.",
                shown(&path, root, scope)
            );
            return Ok(());
        }
    }
    let mut config = if path.exists() {
        read_json(&path).context("Failed to read .cursor/hooks.json.")?
    } else {
        serde_json::json!({})
    };
    config["version"] = serde_json::json!(1);
    let events = config
        .as_object_mut()
        .context("Cursor hook configuration root must be a JSON object.")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let entries = events
        .as_object_mut()
        .context("Cursor hook configuration 'hooks' must be an object.")?
        .entry("beforeShellExecution")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .context("Cursor beforeShellExecution hooks must be an array.")?;
    if !entries.iter().any(|entry| {
        entry.get("command").and_then(serde_json::Value::as_str) == Some("stashbase agent hooks")
    }) {
        entries.push(serde_json::json!({
            "command": "stashbase agent hooks",
            "matcher": "(npm\\s+(install|i|ci)|bun\\s+(add|install)|pnpm\\s+(add|install|i)|yarn\\s+(add|install))(?:\\s+\\S+)?",
            "failClosed": true
        }));
    }
    write_json(&path, &config)?;
    println!(
        "Installed {} Cursor dependency hook in {}",
        scope.label(),
        shown(&path, root, scope)
    );
    Ok(())
}

fn uninstall_cursor_hook(root: &Path, scope: HookScope) -> Result<()> {
    let path = root.join(".cursor/hooks.json");
    if !path.exists() {
        println!("No Cursor dependency hook found in {}", shown(&path, root, scope));
        return Ok(());
    }
    let mut config = read_json(&path).context("Failed to read .cursor/hooks.json.")?;
    if !remove_cursor_hook(&mut config) {
        println!("No Cursor dependency hook found in {}", shown(&path, root, scope));
        return Ok(());
    }
    write_json(&path, &config)?;
    println!(
        "Removed {} Cursor dependency hook from {}",
        scope.label(),
        shown(&path, root, scope)
    );
    Ok(())
}

fn has_cursor_dependency_hook(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let config = read_json(path)?;
    Ok(config
        .pointer("/hooks/beforeShellExecution")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .any(|entry| {
            entry.get("command").and_then(serde_json::Value::as_str)
                == Some("stashbase agent hooks")
        }))
}

fn remove_cursor_hook(config: &mut serde_json::Value) -> bool {
    let Some(entries) = config
        .pointer_mut("/hooks/beforeShellExecution")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return false;
    };
    let before = entries.len();
    entries.retain(|entry| {
        entry.get("command").and_then(serde_json::Value::as_str) != Some("stashbase agent hooks")
    });
    before != entries.len()
}

fn install_claude_hook(root: &Path, scope: HookScope) -> Result<()> {
    let path = root.join(".claude/settings.json");
    let other_path = match scope {
        HookScope::Global => git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
            .map(|root| root.join(".claude/settings.json")),
        HookScope::Project => {
            directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".claude/settings.json"))
        }
        HookScope::Docker => None,
    };
    if let Some(other_path) = other_path
        .as_deref()
        .filter(|other_path| *other_path != path)
    {
        if has_dependency_hook(other_path)? {
            println!(
                "Dependency hook already exists in the other Claude configuration scope; remove it before installing in {}.",
                shown(&path, root, scope)
            );
            return Ok(());
        }
    }
    let mut settings = if path.exists() {
        read_json(&path).context("Failed to read .claude/settings.json.")?
    } else {
        serde_json::json!({})
    };
    add_tool_hook(
        &mut settings,
        "PreToolUse",
        serde_json::json!({
            "matcher": "Bash",
            "hooks": [{
                "type": "command",
                "if": "Bash(npm install *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(npm i *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(bun add *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(bun install *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(pnpm i *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(pnpm add *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(pnpm install *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(yarn add *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(yarn install *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(npm install)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(npm i)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(npm ci)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(bun install)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(pnpm install)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(yarn install)",
                "command": "stashbase agent hooks"
            }]
        }),
    )?;
    write_json(&path, &settings)?;
    println!(
        "Installed {} Claude dependency hook in {}",
        scope.label(),
        shown(&path, root, scope)
    );
    Ok(())
}

fn install_codex_hook(root: &Path, scope: HookScope) -> Result<()> {
    let directory = codex_directory(root, scope);
    let other_directory = match scope {
        HookScope::Global => git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
            .map(|root| root.join(".codex")),
        HookScope::Project => directories::BaseDirs::new()
            .map(|dirs| codex_directory(dirs.home_dir(), HookScope::Global)),
        HookScope::Docker => None,
    };
    if let Some(other_directory) = other_directory
        .as_deref()
        .filter(|other_directory| *other_directory != directory)
    {
        if has_dependency_hook(&other_directory.join("hooks.json"))? {
            println!(
                "Dependency hook already exists in the other Codex configuration scope; remove it before installing in {}.",
                shown(&directory, root, scope)
            );
            return Ok(());
        }
    }
    fs::create_dir_all(&directory)?;
    let path = directory.join("hooks.json");
    let mut hooks = if path.exists() {
        read_json(&path).context("Failed to read .codex/hooks.json.")?
    } else {
        serde_json::json!({})
    };
    add_tool_hook(
        &mut hooks,
        "PreToolUse",
        serde_json::json!({
            "matcher": "Bash",
            "hooks": [{
                "type": "command",
                "command": "stashbase agent hooks"
            }]
        }),
    )?;
    write_json(&path, &hooks)?;
    enable_codex_hooks(&directory.join("config.toml"))?;
    println!(
        "Installed {} Codex dependency hook in {}",
        scope.label(),
        shown(&path, root, scope)
    );
    Ok(())
}

fn enable_codex_hooks(path: &Path) -> Result<()> {
    let contents = path
        .exists()
        .then(|| fs::read_to_string(path))
        .transpose()?
        .unwrap_or_default();
    if contents.is_empty() {
        fs::write(path, "[features]\nhooks = true\n")?;
        return Ok(());
    }
    let _: toml::Value =
        toml::from_str(&contents).context("Failed to parse .codex/config.toml.")?;
    let mut lines: Vec<String> = contents.lines().map(str::to_owned).collect();
    let feature_start = lines.iter().position(|line| line.trim() == "[features]");
    if let Some(start) = feature_start {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.trim_start().starts_with('['))
            .map(|(index, _)| index)
            .unwrap_or(lines.len());
        if let Some(index) = (start + 1..end).find(|&index| {
            lines[index].trim_start().starts_with("hooks =")
                || lines[index].trim_start().starts_with("codex_hooks =")
        }) {
            lines[index] = "hooks = true".to_owned();
        } else {
            lines.insert(start + 1, "hooks = true".to_owned());
        }
    } else {
        lines.extend([
            String::new(),
            "[features]".to_owned(),
            "hooks = true".to_owned(),
        ]);
    }
    fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

fn add_tool_hook(
    config: &mut serde_json::Value,
    event: &str,
    hook: serde_json::Value,
) -> Result<()> {
    let hooks = config
        .as_object_mut()
        .context("Hook configuration root must be a JSON object.")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let events = hooks
        .as_object_mut()
        .context("Hook configuration 'hooks' must be a JSON object.")?
        .entry(event)
        .or_insert_with(|| serde_json::json!([]));
    let entries = events
        .as_array_mut()
        .context("Existing PostToolUse hook configuration must be a JSON array.")?;
    if !entries.iter().any(|entry| entry == &hook) {
        entries.push(hook);
    }
    Ok(())
}

fn remove_tool_hook(config: &mut serde_json::Value) -> bool {
    let Some(events) = config.pointer_mut("/hooks") else {
        return false;
    };
    let Some(events) = events.as_object_mut() else {
        return false;
    };
    let mut removed = false;
    for entries in events.values_mut() {
        let Some(entries) = entries.as_array_mut() else {
            continue;
        };
        entries.retain_mut(|entry| {
            let Some(hooks) = entry
                .get_mut("hooks")
                .and_then(serde_json::Value::as_array_mut)
            else {
                return true;
            };
            let hook_count = hooks.len();
            hooks.retain(|hook| {
                hook.get("command").and_then(serde_json::Value::as_str)
                    != Some("stashbase agent hooks")
            });
            removed |= hooks.len() != hook_count;
            !hooks.is_empty()
        });
    }
    removed
}

fn has_dependency_hook(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let config = read_json(path)?;
    Ok(config
        .pointer("/hooks")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|events| events.values())
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .filter_map(|entry| entry.get("hooks"))
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .any(|hook| {
            hook.get("command").and_then(serde_json::Value::as_str) == Some("stashbase agent hooks")
        }))
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        apply, codex_directory, install_claude_hook, install_cursor_hook,
        parse_preinstall_dependencies, remove_cursor_hook, remove_tool_hook, shown, HookAction,
        HookScope,
    };
    use crate::cmd::deps::HookAgent;
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn docker_scope_installs_into_the_staged_sandbox_home() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("stashbase-docker-hook-test-{suffix}"));

        for agent in [HookAgent::Claude, HookAgent::Codex, HookAgent::Cursor] {
            apply(agent, HookAction::Install, &root, HookScope::Docker).unwrap();
            apply(agent, HookAction::Check, &root, HookScope::Docker).unwrap();
        }

        assert!(fs::read_to_string(root.join(".claude/settings.json"))
            .unwrap()
            .contains("stashbase agent hooks"));
        // Never CODEX_HOME: the host's value means nothing inside the sandbox.
        assert_eq!(codex_directory(&root, HookScope::Docker), root.join(".codex"));
        assert!(root.join(".codex/hooks.json").exists());
        assert!(root.join(".cursor/hooks.json").exists());
        assert_eq!(
            shown(&root.join(".claude/settings.json"), &root, HookScope::Docker),
            Path::new("/home/agent/.claude/settings.json").display().to_string()
        );

        apply(HookAgent::Claude, HookAction::Uninstall, &root, HookScope::Docker).unwrap();
        assert!(apply(HookAgent::Claude, HookAction::Check, &root, HookScope::Docker).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_multiple_preinstall_packages_with_optional_versions() {
        for command in [
            "bun add lodash minimist@1.2.5",
            "npm i lodash",
            "pnpm i lodash",
            "yarn add lodash",
        ] {
            let dependencies = parse_preinstall_dependencies(&serde_json::json!({
                "tool_input": { "command": command }
            }))
            .unwrap()
            .unwrap();
            assert!(!dependencies.is_empty());
        }
        let dependencies = parse_preinstall_dependencies(&serde_json::json!({
            "tool_input": { "command": "bun add lodash minimist@1.2.5" }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0].name, "lodash");
        assert_eq!(dependencies[0].version, None);
        assert_eq!(dependencies[1].version.as_deref(), Some("1.2.5"));
    }

    #[test]
    fn parses_all_chained_preinstall_commands() {
        let dependencies = parse_preinstall_dependencies(&serde_json::json!({
            "tool_input": {
                "command": "npm install safe && npm install malicious || bun add another; pnpm i final"
            }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(dependencies.len(), 4);
        assert_eq!(dependencies[0].name, "safe");
        assert_eq!(dependencies[1].name, "malicious");
        assert_eq!(dependencies[2].name, "another");
        assert_eq!(dependencies[3].name, "final");
    }

    #[test]
    fn ignores_commands_without_installs() {
        assert!(parse_preinstall_dependencies(&serde_json::json!({
            "tool_input": { "command": "npm run build" }
        }))
        .unwrap()
        .is_none());
    }

    #[test]
    fn scans_root_dependencies_at_lockfile_versions() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("stashbase-root-deps-test-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"lodash":"^4.17.0"},"devDependencies":{"vitest":"^2.0.0"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("package-lock.json"),
            r#"{"lockfileVersion":3,"packages":{"node_modules/lodash":{"version":"4.17.21"},"node_modules/vitest":{"version":"2.1.9"}}}"#,
        )
        .unwrap();

        let dependencies = parse_preinstall_dependencies(&serde_json::json!({
            "cwd": root,
            "tool_input": { "command": "npm ci" }
        }))
        .unwrap()
        .unwrap();

        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0].version.as_deref(), Some("4.17.21"));
        assert_eq!(dependencies[1].version.as_deref(), Some("2.1.9"));

        fs::remove_file(root.join("package-lock.json")).unwrap();
        fs::write(
            root.join("package-lock.json"),
            r#"{"lockfileVersion":1,"dependencies":{"lodash":{"version":"4.17.21"},"vitest":{"version":"2.1.9"}}}"#,
        )
        .unwrap();
        let dependencies = parse_preinstall_dependencies(&serde_json::json!({
            "cwd": root,
            "tool_input": { "command": "npm ci" }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(dependencies[0].version.as_deref(), Some("4.17.21"));
        assert_eq!(dependencies[1].version.as_deref(), Some("2.1.9"));

        fs::remove_file(root.join("package-lock.json")).unwrap();
        fs::write(
            root.join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      lodash:\n        specifier: ^4.17.0\n        version: 4.17.21\n    devDependencies:\n      vitest:\n        specifier: ^2.0.0\n        version: 2.1.9\n",
        )
        .unwrap();
        let dependencies = parse_preinstall_dependencies(&serde_json::json!({
            "cwd": root,
            "tool_input": { "command": "pnpm install" }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(dependencies[0].version.as_deref(), Some("4.17.21"));
        assert_eq!(dependencies[1].version.as_deref(), Some("2.1.9"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_claude_hooks_for_all_package_manager_aliases() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("stashbase-claude-hook-test-{suffix}"));

        install_claude_hook(&root, HookScope::Global).unwrap();
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".claude/settings.json")).unwrap())
                .unwrap();
        let matchers = settings["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|hook| hook["if"].as_str())
            .collect::<Vec<_>>();

        assert!(matchers.contains(&"Bash(bun install *)"));
        assert!(matchers.contains(&"Bash(pnpm install *)"));
        assert!(matchers.contains(&"Bash(yarn add *)"));
        assert!(matchers.contains(&"Bash(yarn install *)"));

        install_cursor_hook(&root, HookScope::Global).unwrap();
        let hooks: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".cursor/hooks.json")).unwrap())
                .unwrap();
        assert!(hooks["hooks"]["beforeShellExecution"][0]["matcher"]
            .as_str()
            .unwrap()
            .contains("yarn\\s+(add|install)"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removes_only_stashbase_hooks() {
        let mut config = serde_json::json!({
            "hooks": {
                "PreToolUse": [{"matcher": "Bash", "hooks": [
                    {"type": "command", "command": "stashbase agent hooks"},
                    {"type": "command", "command": "other-hook"}
                ]}],
                "PostToolUse": [{"hooks": [{"type": "command", "command": "stashbase agent hooks"}]}]
            }
        });
        assert!(remove_tool_hook(&mut config));
        assert_eq!(
            config["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "other-hook"
        );
        assert!(config["hooks"]["PostToolUse"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(!remove_tool_hook(&mut config));
    }

    #[test]
    fn removes_only_cursor_dependency_hooks() {
        let mut config = serde_json::json!({
            "version": 1,
            "hooks": {
                "beforeShellExecution": [
                    {"command": "stashbase agent hooks"},
                    {"command": "custom hook"}
                ]
            }
        });
        assert!(remove_cursor_hook(&mut config));
        assert_eq!(
            config["hooks"]["beforeShellExecution"],
            serde_json::json!([{"command": "custom hook"}])
        );
        assert!(!remove_cursor_hook(&mut config));
    }
}

fn read_json(path: &Path) -> Result<serde_json::Value> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}
