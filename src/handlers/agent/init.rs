//! Safe scaffolding for Agent Proxy profiles, repository-local or global.

use std::{
    env, fs,
    io::IsTerminal,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use super::init_wizard::{run_wizard, WizardTargets};
use crate::{
    cmd::agent::AgentInitCommand,
    config::config::{
        display_global_path, global_agent_profiles_dir, DIRECTORY_AGENT_PROFILES_DIR,
    },
    utils::output::get_formatted_json_string,
};

const PROFILE_TEMPLATE: &str = r#"# Stashbase Agent Proxy profile.
# Add only destinations the agent genuinely needs to contact.
egress_hosts = []

# Enable installed dependency and secret-scan hooks for this profile.
# allow_hooks = ["dependency_check", "secret_scan"]

# Optional: allow local test servers and Unix-socket IPC (macOS only).
# Linux keeps loopback available for the embedded proxy; this setting has no effect there.
# allow_network_listeners = true

# Optional: protect sensitive files from agent reads and writes.
# [filesystem]
# deny_read = ["~/.ssh", "~/.aws", ".env"]
# deny_write = ["~/.ssh", "~/.aws", ".git"]

# Optional: non-sensitive config for the agent, stored in plaintext.
# Credentials belong in [secrets]; values that look like one are flagged.
# [config_env]
# NODE_ENV = "development"
# PORT = 3000

# Grant a Stashbase secret: uncomment and replace SECRET_NAME.
# [secrets.SECRET_NAME]

# Optional named HTTP MCP server and tool allowlist.
# [mcp_servers.example]
# url = "https://mcp.example.com/mcp"
# binding = "SECRET_NAME"
# header = "X-API-Key"
# value_template = "{value}"
# allow_tools = ["search"]

# Example: permit one read-only HTTP action for this credential.
# [[secrets.SECRET_NAME.rules]]
# effect = "allow"
# hosts = ["api.example.com"]
# methods = ["GET"]
# paths = ["/v1/resource/*"]
"#;

#[derive(Debug, Serialize)]
struct AgentInitReport {
    profile: String,
    path: String,
    overwritten: bool,
}

pub fn handle_agent_init_command(
    command: AgentInitCommand,
    silent: bool,
    json: bool,
) -> Result<()> {
    let Some(profile) = command.profile else {
        return run_interactive(command.global, command.force, json);
    };
    validate_profile_name(&profile)?;
    let path = profile_path(&profile, command.global)?;
    let existed = path.exists();
    if existed && !command.force {
        bail!(
            "Refusing to overwrite existing agent profile '{}'. Pass --force to replace it.",
            path.display()
        );
    }
    write_profile(&path, PROFILE_TEMPLATE)?;

    let report = AgentInitReport {
        profile,
        path: shown_path(&path, command.global),
        overwritten: existed,
    };
    if !silent {
        println!();
    }
    if json {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else if report.overwritten {
        println!("Replaced agent profile: {}", report.path);
    } else {
        println!("Created agent profile: {}", report.path);
    }
    Ok(())
}

fn run_interactive(global: bool, force: bool, json: bool) -> Result<()> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if json || !interactive {
        bail!(
            "Interactive setup needs a terminal. Pass a profile name to write a starter template: \
             stashbase agent init <PROFILE>"
        );
    }
    let outcome = run_wizard(WizardTargets {
        preset_global: global,
        force,
        resolve: &profile_path,
        display: &shown_path,
        validate_name: &validate_profile_name,
    })?;
    let Some(outcome) = outcome else {
        let _ = dialoguer::console::Term::stdout().show_cursor();
        println!("Cancelled. No profile was written.");
        return Ok(());
    };
    write_profile(&outcome.path, &outcome.contents)?;
    let shown = shown_path(&outcome.path, outcome.global);
    let verb = if outcome.overwrite {
        "Replaced"
    } else {
        "Created"
    };
    println!("{verb} agent profile: {shown}");
    println!("Edit it any time to add rules, MCP servers or hooks.");
    println!(
        "\nStart the agent:\n  stashbase agent run --profile {} -- {}",
        outcome.profile, outcome.agent_command
    );
    Ok(())
}

fn profile_path(profile: &str, global: bool) -> Result<PathBuf> {
    let directory = if global {
        global_agent_profiles_dir()?
    } else {
        env::current_dir()
            .context("Could not determine the current directory for the agent profile.")?
            .join(DIRECTORY_AGENT_PROFILES_DIR)
    };
    Ok(directory.join(format!("{profile}.toml")))
}

fn shown_path(path: &Path, global: bool) -> String {
    if global {
        display_global_path(path)
    } else {
        display_path(path)
    }
}

fn write_profile(path: &Path, contents: &str) -> Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).with_context(|| {
            format!(
                "Could not create agent profiles directory '{}'.",
                directory.display()
            )
        })?;
    }
    fs::write(path, contents)
        .with_context(|| format!("Could not write agent profile '{}'.", path.display()))
}

fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.contains(std::path::MAIN_SEPARATOR)
    {
        bail!("Agent profile name '{name}' must be a plain file name.");
    }
    Ok(())
}

fn display_path(path: &std::path::Path) -> String {
    let current_directory = env::current_dir().ok();
    current_directory
        .as_deref()
        .and_then(|directory| path.strip_prefix(directory).ok())
        .map(|relative| format!("./{}", relative.display()))
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::{validate_profile_name, PROFILE_TEMPLATE};

    #[test]
    fn template_starts_closed_and_includes_a_generic_rule() {
        assert!(PROFILE_TEMPLATE.contains("egress_hosts = []"));
        assert!(PROFILE_TEMPLATE.contains("allow_network_listeners = true"));
        assert!(
            PROFILE_TEMPLATE.contains("# allow_hooks = [\"dependency_check\", \"secret_scan\"]")
        );
        assert!(PROFILE_TEMPLATE.contains("# [secrets.SECRET_NAME]"));
        assert!(PROFILE_TEMPLATE.contains("# [[secrets.SECRET_NAME.rules]]"));
    }

    #[test]
    fn template_parses_without_placeholder_secrets() {
        let profile: crate::models::agent::AgentProfile = toml::from_str(PROFILE_TEMPLATE).unwrap();
        assert!(profile.secrets.bindings.is_empty());
    }

    #[test]
    fn template_secret_example_is_valid_when_uncommented() {
        let uncommented: String = PROFILE_TEMPLATE
            .lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(rest)
                    if rest.starts_with("[secrets.")
                        || rest.starts_with("[[secrets.")
                        || ["effect", "hosts", "methods", "paths"]
                            .iter()
                            .any(|key| rest.starts_with(&format!("{key} ="))) =>
                {
                    rest
                }
                _ => line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let profile: crate::models::agent::AgentProfile = toml::from_str(&uncommented).unwrap();
        assert_eq!(profile.secrets.bindings["SECRET_NAME"].rules.len(), 1);
    }

    #[test]
    fn template_config_env_example_is_valid_when_uncommented() {
        assert!(PROFILE_TEMPLATE.contains("# [config_env]"));
        let uncommented = PROFILE_TEMPLATE
            .replace("# [config_env]", "[config_env]")
            .replace("# NODE_ENV = ", "NODE_ENV = ")
            .replace("# PORT = ", "PORT = ");
        let profile: crate::models::agent::AgentProfile = toml::from_str(&uncommented).unwrap();
        assert_eq!(profile.config_env["NODE_ENV"].value(), "development");
        assert_eq!(profile.config_env["PORT"].value(), "3000");
    }

    #[test]
    fn rejects_profile_names_that_could_escape_the_profiles_directory() {
        assert!(validate_profile_name("codex").is_ok());
        assert!(validate_profile_name("../codex").is_err());
        assert!(validate_profile_name("nested/codex").is_err());
    }
}
