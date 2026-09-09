use anyhow::{bail, Context, Result};
use std::{fs, io::Read, path::Path};

use crate::{
    api::dependencies::check_batch,
    cmd::deps::{AgentHooksCommand, AgentHooksSubcommand, HookAgent},
    models::dependencies::{DependencyCheckRequest, DependencyDecision},
};

pub async fn handle_agent_hooks_commands(cmd: AgentHooksCommand, api_key: String) -> Result<()> {
    match cmd.subcommand {
        Some(AgentHooksSubcommand::Install(args)) => return install_hook(args.agent, args.global),
        Some(AgentHooksSubcommand::Remove(args)) => return uninstall_hook(args.agent, args.global),
        None => return handle_hook(api_key).await,
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
    for segment in command.split(['&', '|', ';']) {
        let words = segment
            .split_whitespace()
            .map(|word| word.trim_matches(['\'', '"']))
            .collect::<Vec<_>>();
        for index in 0..words.len().saturating_sub(1) {
            let is_manager = matches!(words[index], "npm" | "bun" | "pnpm");
            let is_install = matches!(
                (words[index], words[index + 1]),
                ("npm", "install" | "i")
                    | ("bun", "add" | "install")
                    | ("pnpm", "add" | "install" | "i")
            );
            if !is_manager || !is_install {
                continue;
            }
            let mut dependencies = Vec::new();
            for spec in words[index + 2..]
                .iter()
                .copied()
                .filter(|word| !word.starts_with('-'))
            {
                dependencies.push(parse_package_spec(spec)?);
            }
            return Ok(Some(dependencies));
        }
    }
    Ok(None)
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

fn install_hook(agent: HookAgent, global: bool) -> Result<()> {
    let root = if global {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .context("Could not determine the home directory for global hooks.")?
    } else {
        git2::Repository::discover(".")
            .context("Hook installation must run inside a git repository.")?
            .workdir()
            .map(Path::to_path_buf)
            .context("Git repository has no working directory.")?
    };
    match agent {
        HookAgent::Claude => install_claude_hook(&root, global),
        HookAgent::Codex => install_codex_hook(&root, global),
        HookAgent::Cursor => install_cursor_hook(&root, global),
    }
}

fn uninstall_hook(agent: HookAgent, global: bool) -> Result<()> {
    let root = if global {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .context("Could not determine the home directory for global hooks.")?
    } else {
        git2::Repository::discover(".")
            .context("Hook removal must run inside a git repository.")?
            .workdir()
            .map(Path::to_path_buf)
            .context("Git repository has no working directory.")?
    };
    match agent {
        HookAgent::Claude => uninstall_claude_hook(&root, global),
        HookAgent::Codex => uninstall_codex_hook(&root, global),
        HookAgent::Cursor => uninstall_cursor_hook(&root, global),
    }
}

fn uninstall_claude_hook(root: &Path, global: bool) -> Result<()> {
    let path = root.join(".claude/settings.json");
    if !path.exists() {
        println!("No Claude dependency hook found in {}", path.display());
        return Ok(());
    }
    let mut settings = read_json(&path).context("Failed to read .claude/settings.json.")?;
    if !remove_tool_hook(&mut settings) {
        println!("No Claude dependency hook found in {}", path.display());
        return Ok(());
    }
    write_json(&path, &settings)?;
    println!(
        "Removed {} Claude dependency hook from {}",
        if global { "global" } else { "project" },
        path.display()
    );
    Ok(())
}

fn uninstall_codex_hook(root: &Path, global: bool) -> Result<()> {
    let directory = if global {
        std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join(".codex"))
    } else {
        root.join(".codex")
    };
    let path = directory.join("hooks.json");
    if !path.exists() {
        println!("No Codex dependency hook found in {}", path.display());
        return Ok(());
    }
    let mut hooks = read_json(&path).context("Failed to read .codex/hooks.json.")?;
    if !remove_tool_hook(&mut hooks) {
        println!("No Codex dependency hook found in {}", path.display());
        return Ok(());
    }
    write_json(&path, &hooks)?;
    println!(
        "Removed {} Codex dependency hook from {}",
        if global { "global" } else { "project" },
        path.display()
    );
    Ok(())
}

fn install_cursor_hook(root: &Path, global: bool) -> Result<()> {
    let directory = root.join(".cursor");
    let other_path = if global {
        git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
            .map(|root| root.join(".cursor/hooks.json"))
    } else {
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".cursor/hooks.json"))
    };
    let path = directory.join("hooks.json");
    if let Some(other_path) = other_path.as_deref().filter(|other| *other != path) {
        if has_cursor_dependency_hook(other_path)? {
            println!(
                "Dependency hook already exists in the other Cursor configuration scope; remove it before installing in {}.",
                path.display()
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
            "matcher": "(npm\\s+(install|i)|bun\\s+(add|install)|pnpm\\s+(add|install|i))\\s+\\S+",
            "failClosed": true
        }));
    }
    write_json(&path, &config)?;
    println!(
        "Installed {} Cursor dependency hook in {}",
        if global { "global" } else { "project" },
        path.display()
    );
    Ok(())
}

fn uninstall_cursor_hook(root: &Path, global: bool) -> Result<()> {
    let path = root.join(".cursor/hooks.json");
    if !path.exists() {
        println!("No Cursor dependency hook found in {}", path.display());
        return Ok(());
    }
    let mut config = read_json(&path).context("Failed to read .cursor/hooks.json.")?;
    if !remove_cursor_hook(&mut config) {
        println!("No Cursor dependency hook found in {}", path.display());
        return Ok(());
    }
    write_json(&path, &config)?;
    println!(
        "Removed {} Cursor dependency hook from {}",
        if global { "global" } else { "project" },
        path.display()
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

fn install_claude_hook(root: &Path, global: bool) -> Result<()> {
    let path = root.join(".claude/settings.json");
    let other_path = if global {
        git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
            .map(|root| root.join(".claude/settings.json"))
    } else {
        directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".claude/settings.json"))
    };
    if let Some(other_path) = other_path
        .as_deref()
        .filter(|other_path| *other_path != path)
    {
        if has_dependency_hook(other_path)? {
            println!(
                "Dependency hook already exists in the other Claude configuration scope; remove it before installing in {}.",
                path.display()
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
                "if": "Bash(pnpm i *)",
                "command": "stashbase agent hooks"
            }, {
                "type": "command",
                "if": "Bash(pnpm add *)",
                "command": "stashbase agent hooks"
            }]
        }),
    )?;
    write_json(&path, &settings)?;
    println!(
        "Installed {} Claude dependency hook in {}",
        if global { "global" } else { "project" },
        path.display()
    );
    Ok(())
}

fn install_codex_hook(root: &Path, global: bool) -> Result<()> {
    let directory = if global {
        std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| root.join(".codex"))
    } else {
        root.join(".codex")
    };
    let other_directory = if global {
        git2::Repository::discover(".")
            .ok()
            .and_then(|repo| repo.workdir().map(Path::to_path_buf))
            .map(|root| root.join(".codex"))
    } else {
        directories::BaseDirs::new().map(|dirs| {
            std::env::var_os("CODEX_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| dirs.home_dir().join(".codex"))
        })
    };
    if let Some(other_directory) = other_directory
        .as_deref()
        .filter(|other_directory| *other_directory != directory)
    {
        if has_dependency_hook(&other_directory.join("hooks.json"))? {
            println!(
                "Dependency hook already exists in the other Codex configuration scope; remove it before installing in {}.",
                directory.display()
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
        if global { "global" } else { "project" },
        path.display()
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
    use super::{parse_preinstall_dependencies, remove_cursor_hook, remove_tool_hook};

    #[test]
    fn parses_multiple_preinstall_packages_with_optional_versions() {
        for command in [
            "bun add lodash minimist@1.2.5",
            "npm i lodash",
            "pnpm i lodash",
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
