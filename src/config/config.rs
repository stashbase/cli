use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;

use crate::models::{
    agent::AgentProfile,
    config::{Config, DirectoryConfig, OutputFormatConfig, UpdateConfig},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
const CONFIG_DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const CONFIG_FILE_MODE: u32 = 0o600;

/// Repository-local, security-sensitive policy for `stashbase agent run`.
pub const DIRECTORY_AGENT_PROFILE_FILE: &str = "stashbase-agent.toml";
/// Scalable repository-local layout: one direct agent profile per file.
pub const DIRECTORY_AGENT_PROFILES_DIR: &str = ".stashbase/agents";

#[derive(Debug, Clone)]
pub struct LoadedDirectoryAgentProfile {
    pub profile: AgentProfile,
    pub source: String,
    pub path: PathBuf,
}

fn ensure_config_dir(config_dir: &Path) -> Result<()> {
    fs::create_dir_all(config_dir)?;
    apply_secure_dir_permissions(config_dir)?;
    Ok(())
}

fn apply_secure_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(CONFIG_FILE_MODE))?;
    }

    Ok(())
}

fn apply_secure_dir_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(CONFIG_DIR_MODE))?;
    }

    Ok(())
}

fn write_config(path: &Path, config: &Config) -> Result<()> {
    let config_string = toml::to_string(config)?;
    fs::write(path, config_string)?;
    apply_secure_file_permissions(path)?;
    Ok(())
}

pub fn create_config(path: &Path) -> Result<String> {
    let new_config = Config::new();

    let toml_string = toml::to_string(&new_config)
        .with_context(|| format!("Could not create toml config file."))?;

    fs::write(path, &toml_string)?;
    apply_secure_file_permissions(path)?;

    Ok(toml_string)
}

pub fn get_config_path() -> Result<PathBuf> {
    let dir_path = ProjectDirs::from("", "", "stashbase");

    match dir_path {
        Some(dirs) => {
            let config_dir = dirs.config_dir();
            ensure_config_dir(config_dir)?;
            // Return the full path to the config file
            Ok(config_dir.join("config.toml"))
        }
        None => bail!("Could not find config directory."),
    }
}

pub fn get_config() -> Result<Config> {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "stashbase") {
        let config_dir = proj_dirs.config_dir();
        let config_file_path = config_dir.join("config.toml");
        let config_file_exists = config_file_path.is_file();

        match config_file_exists {
            true => {
                apply_secure_file_permissions(&config_file_path)?;
                let content = fs::read_to_string(&config_file_path)?;
                let data =
                    toml::from_str::<Config>(&content).context("Could not parse config file.")?;

                Ok(data)
            }
            false => {
                ensure_config_dir(config_dir)?;
                let new_config = create_config(&config_file_path)?;
                let data = toml::from_str::<Config>(&new_config)?;

                Ok(data)
            }
        }
    } else {
        bail!("Could not find config directory.")
    }
}

/// Load a repository-local profile from `.stashbase/agents/<name>.toml`, falling
/// back to the legacy `stashbase-agent.toml` layout. These files never create
/// config directories and are ignored when absent.
pub fn get_directory_agent_profile(
    profile_name: &str,
) -> Result<Option<LoadedDirectoryAgentProfile>> {
    let current_dir = std::env::current_dir()?;
    get_directory_agent_profile_from_dir(&current_dir, profile_name)
}

/// Load one direct profile file for explicit CI or automation use.
pub fn get_explicit_agent_profile(path: &Path) -> Result<LoadedDirectoryAgentProfile> {
    let path = path
        .canonicalize()
        .with_context(|| format!("Could not resolve agent policy file '{}'.", path.display()))?;
    if !path.is_file() {
        bail!("Agent policy file '{}' is not a file.", path.display());
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Could not read agent policy file '{}'.", path.display()))?;
    let profile = toml::from_str::<AgentProfile>(&content)
        .with_context(|| format!("Could not parse agent policy file '{}'.", path.display()))?;
    Ok(LoadedDirectoryAgentProfile {
        profile,
        source: path.display().to_string(),
        path,
    })
}

/// List every repository-local profile from the scalable and legacy layouts.
/// A duplicate name is rejected so callers never have to infer precedence.
pub fn get_directory_agent_profiles() -> Result<Vec<(String, LoadedDirectoryAgentProfile)>> {
    let current_dir = std::env::current_dir()?;
    get_directory_agent_profiles_from_dir(&current_dir)
}

fn get_directory_agent_profiles_from_dir(
    directory: &Path,
) -> Result<Vec<(String, LoadedDirectoryAgentProfile)>> {
    let mut profiles = std::collections::BTreeMap::new();
    let agents_directory = directory.join(DIRECTORY_AGENT_PROFILES_DIR);
    if agents_directory.is_dir() {
        for entry in fs::read_dir(&agents_directory).with_context(|| {
            format!(
                "Could not read agent profiles directory '{}'.",
                agents_directory.display()
            )
        })? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_safe_profile_file_name(name) {
                continue;
            }
            let content = fs::read_to_string(&path).with_context(|| {
                format!("Could not read agent profile file '{}'.", path.display())
            })?;
            let profile = toml::from_str::<AgentProfile>(&content).with_context(|| {
                format!("Could not parse agent profile file '{}'.", path.display())
            })?;
            profiles.insert(
                name.to_owned(),
                LoadedDirectoryAgentProfile {
                    profile,
                    source: display_directory_profile_path(directory, &path),
                    path,
                },
            );
        }
    }

    let legacy_path = directory.join(DIRECTORY_AGENT_PROFILE_FILE);
    if legacy_path.is_file() {
        let content = fs::read_to_string(&legacy_path).with_context(|| {
            format!(
                "Could not read agent profile file '{}'.",
                legacy_path.display()
            )
        })?;
        let legacy: DirectoryConfig = toml::from_str(&content).with_context(|| {
            format!(
                "Could not parse agent profile file '{}'.",
                legacy_path.display()
            )
        })?;
        for (name, profile) in legacy.agent_profiles.unwrap_or_default() {
            if profiles.contains_key(&name) {
                bail!(
                    "Agent profile '{name}' is defined in both '{}' and '{}'. Remove one definition.",
                    agents_directory.join(format!("{name}.toml")).display(),
                    legacy_path.display(),
                );
            }
            profiles.insert(
                name,
                LoadedDirectoryAgentProfile {
                    profile,
                    source: format!("./{DIRECTORY_AGENT_PROFILE_FILE}"),
                    path: legacy_path.clone(),
                },
            );
        }
    }
    Ok(profiles.into_iter().collect())
}

fn get_directory_agent_profile_from_dir(
    directory: &Path,
    profile_name: &str,
) -> Result<Option<LoadedDirectoryAgentProfile>> {
    if !is_safe_profile_file_name(profile_name) {
        bail!(
            "Agent profile name '{profile_name}' must be a plain file name when using directory profiles."
        );
    }

    let modern_path = directory
        .join(DIRECTORY_AGENT_PROFILES_DIR)
        .join(format!("{profile_name}.toml"));
    let modern_profile = if modern_path.is_file() {
        let content = fs::read_to_string(&modern_path).with_context(|| {
            format!(
                "Could not read agent profile file '{}'.",
                modern_path.display()
            )
        })?;
        Some(toml::from_str::<AgentProfile>(&content).with_context(|| {
            format!(
                "Could not parse agent profile file '{}'.",
                modern_path.display()
            )
        })?)
    } else {
        None
    };

    let legacy_path = directory.join(DIRECTORY_AGENT_PROFILE_FILE);
    let legacy_profile = if legacy_path.is_file() {
        let content = fs::read_to_string(&legacy_path).with_context(|| {
            format!(
                "Could not read agent profile file '{}'.",
                legacy_path.display()
            )
        })?;
        let config: DirectoryConfig = toml::from_str(&content).with_context(|| {
            format!(
                "Could not parse agent profile file '{}'.",
                legacy_path.display()
            )
        })?;
        config
            .agent_profiles
            .and_then(|profiles| profiles.get(profile_name).cloned())
    } else {
        None
    };

    match (modern_profile, legacy_profile) {
        (Some(_), Some(_)) => bail!(
            "Agent profile '{profile_name}' is defined in both '{}' and '{}'. Remove one definition.",
            modern_path.display(),
            legacy_path.display(),
        ),
        (Some(profile), None) => Ok(Some(LoadedDirectoryAgentProfile {
            profile,
            source: display_directory_profile_path(directory, &modern_path),
            path: modern_path,
        })),
        (None, Some(profile)) => Ok(Some(LoadedDirectoryAgentProfile {
            profile,
            source: format!("./{DIRECTORY_AGENT_PROFILE_FILE}"),
            path: legacy_path,
        })),
        (None, None) => Ok(None),
    }
}

fn is_safe_profile_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.contains(std::path::MAIN_SEPARATOR)
}

fn display_directory_profile_path(directory: &Path, path: &Path) -> String {
    path.strip_prefix(directory)
        .map(|relative| format!("./{}", relative.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

pub fn update_config(args: UpdateConfig) -> Result<()> {
    let config_path = get_config_path()?;
    let mut config = get_config()?;

    let UpdateConfig {
        api_key,
        output_format,
        expand_refs,
    } = args;

    if let Some(new_api_key) = api_key {
        config.api_key = if new_api_key.is_empty() {
            None
        } else {
            Some(new_api_key)
        };
    }

    if let Some(new_expand_refs) = expand_refs {
        config.expand_refs = Some(new_expand_refs);
    }

    if let Some(output_format) = output_format {
        let mut new_format_config = match config.ouput_format {
            Some(o) => o,
            None => OutputFormatConfig::new(),
        };

        if let Some(general_format) = output_format.general {
            new_format_config.general = Some(general_format);
        }

        if let Some(secrets_format) = output_format.secrets {
            new_format_config.secrets = Some(secrets_format);
        }

        config.ouput_format = Some(new_format_config);
    }

    write_config(&config_path, &config)
}

pub fn clear_legacy_api_key() -> Result<()> {
    let config_path = get_config_path()?;
    let mut config = get_config()?;
    config.api_key = None;
    write_config(&config_path, &config)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        get_directory_agent_profile_from_dir, get_directory_agent_profiles_from_dir,
        get_explicit_agent_profile, DIRECTORY_AGENT_PROFILES_DIR,
    };

    fn temporary_directory() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "stashbase-agent-profile-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn loads_direct_profile_from_agents_directory() {
        let directory = temporary_directory();
        let agents = directory.join(DIRECTORY_AGENT_PROFILES_DIR);
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("codex.toml"),
            "egress_hosts = [\"api.github.com\"]\n[secrets.GITHUB_TOKEN]\nhosts = [\"api.github.com\"]\n",
        )
        .unwrap();

        let loaded = get_directory_agent_profile_from_dir(&directory, "codex")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.source, "./.stashbase/agents/codex.toml");
        assert_eq!(
            loaded.profile.secrets["GITHUB_TOKEN"].hosts,
            ["api.github.com"]
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn falls_back_to_legacy_directory_profile() {
        let directory = temporary_directory();
        fs::write(
            directory.join("stashbase-agent.toml"),
            "[agent_profiles.codex]\negress_hosts = [\"api.github.com\"]\n",
        )
        .unwrap();

        let loaded = get_directory_agent_profile_from_dir(&directory, "codex")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.source, "./stashbase-agent.toml");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_profile_defined_in_both_directory_layouts() {
        let directory = temporary_directory();
        let agents = directory.join(DIRECTORY_AGENT_PROFILES_DIR);
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("codex.toml"),
            "egress_hosts = [\"api.github.com\"]\n",
        )
        .unwrap();
        fs::write(
            directory.join("stashbase-agent.toml"),
            "[agent_profiles.codex]\negress_hosts = [\"api.github.com\"]\n",
        )
        .unwrap();

        let error = get_directory_agent_profile_from_dir(&directory, "codex")
            .unwrap_err()
            .to_string();
        assert!(error.contains("defined in both"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn lists_direct_and_legacy_directory_profiles() {
        let directory = temporary_directory();
        let agents = directory.join(DIRECTORY_AGENT_PROFILES_DIR);
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("codex.toml"),
            "egress_hosts = [\"api.github.com\"]\n",
        )
        .unwrap();
        fs::write(
            directory.join("stashbase-agent.toml"),
            "[agent_profiles.claude]\negress_hosts = [\"api.anthropic.com\"]\n",
        )
        .unwrap();

        let profiles = get_directory_agent_profiles_from_dir(&directory).unwrap();
        assert_eq!(
            profiles
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["claude", "codex"]
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_an_explicit_direct_profile_file() {
        let directory = temporary_directory();
        let path = directory.join("ci-policy.toml");
        fs::write(&path, "egress_hosts = [\"api.github.com\"]\n").unwrap();

        let loaded = get_explicit_agent_profile(&path).unwrap();
        assert_eq!(loaded.path, path.canonicalize().unwrap());
        assert_eq!(
            loaded.source,
            path.canonicalize().unwrap().display().to_string()
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
