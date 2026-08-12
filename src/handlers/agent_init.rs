//! Safe scaffolding for repository-local Agent Proxy profiles.

use std::{env, fs};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::{
    cmd::agent::AgentInitCommand, config::config::DIRECTORY_AGENT_PROFILES_DIR,
    utils::output::get_formatted_json_string,
};

const PROFILE_TEMPLATE: &str = r#"# Stashbase Agent Proxy profile.
# Add only destinations the agent genuinely needs to contact.
egress_hosts = []

# Replace SECRET_NAME with the Stashbase secret to grant.
[secrets.SECRET_NAME]

# Example: permit one read-only HTTP action for this credential.
[[secrets.SECRET_NAME.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET"]
paths = ["/v1/resource/*"]
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
    validate_profile_name(&command.profile)?;
    let directory = env::current_dir()
        .context("Could not determine the current directory for the agent profile.")?
        .join(DIRECTORY_AGENT_PROFILES_DIR);
    let path = directory.join(format!("{}.toml", command.profile));
    let existed = path.exists();
    if existed && !command.force {
        bail!(
            "Refusing to overwrite existing agent profile '{}'. Pass --force to replace it.",
            path.display()
        );
    }

    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "Could not create agent profiles directory '{}'.",
            directory.display()
        )
    })?;
    fs::write(&path, PROFILE_TEMPLATE)
        .with_context(|| format!("Could not write agent profile '{}'.", path.display()))?;

    let report = AgentInitReport {
        profile: command.profile,
        path: display_path(&path),
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
        assert!(PROFILE_TEMPLATE.contains("[secrets.SECRET_NAME]"));
        assert!(PROFILE_TEMPLATE.contains("[[secrets.SECRET_NAME.rules]]"));
    }

    #[test]
    fn rejects_profile_names_that_could_escape_the_profiles_directory() {
        assert!(validate_profile_name("codex").is_ok());
        assert!(validate_profile_name("../codex").is_err());
        assert!(validate_profile_name("nested/codex").is_err());
    }
}
