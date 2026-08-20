use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;

use crate::models::{
    agent::AgentProfile,
    config::{Config, OutputFormatConfig, ProfileConfig, UpdateConfig},
};

use crate::utils::env::get_env_var;

pub const DEFAULT_PROFILE: &str = "default";
const PROFILE_ENV_VAR: &str = "STASHBASE_PROFILE";

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
const CONFIG_DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const CONFIG_FILE_MODE: u32 = 0o600;

/// Repository-local, security-sensitive policy for `stashbase agent run`.
/// Each direct profile is stored in its own file.
pub const DIRECTORY_AGENT_PROFILES_DIR: &str = ".stashbase/agents";
const REMOVED_DIRECTORY_AGENT_PROFILE_FILE: &str = "stashbase-agent.toml";

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

pub fn resolve_profile_name(config: &Config, cli_profile: Option<&str>) -> Result<String> {
    let profile = cli_profile
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .map(str::to_owned)
        .or_else(|| get_env_var(PROFILE_ENV_VAR))
        .or_else(|| config.default_profile.clone())
        .unwrap_or_else(|| DEFAULT_PROFILE.to_owned());

    validate_profile_name(&profile)?;
    if profile != DEFAULT_PROFILE
        && !config
            .profiles
            .as_ref()
            .is_some_and(|profiles| profiles.contains_key(&profile))
    {
        bail!("Profile '{profile}' was not found. Run 'stashbase config profile list' to see available profiles.");
    }
    Ok(profile)
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("Profile name '{name}' must contain only letters, numbers, hyphens, or underscores.");
    }
    Ok(())
}

pub fn add_profile(name: &str, workspace: Option<String>) -> Result<()> {
    validate_profile_name(name)?;
    if name == DEFAULT_PROFILE {
        bail!("'default' is the implicit backwards-compatible profile and cannot be added. Use 'config api-key set' to manage its key.");
    }
    let config_path = get_config_path()?;
    let mut config = get_config()?;
    let profiles = config.profiles.get_or_insert_with(Default::default);
    let existing_workspace = profiles
        .get(name)
        .and_then(|profile| profile.workspace.clone());
    profiles.insert(
        name.to_owned(),
        ProfileConfig {
            workspace: workspace
                .filter(|workspace| !workspace.trim().is_empty())
                .or(existing_workspace),
        },
    );
    write_config(&config_path, &config)
}

pub fn remove_profile(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    if name == DEFAULT_PROFILE {
        bail!("The implicit 'default' profile cannot be removed. Use 'config api-key set' to replace its key.");
    }
    let config_path = get_config_path()?;
    let mut config = get_config()?;
    let removed = config
        .profiles
        .as_mut()
        .and_then(|profiles| profiles.remove(name));
    if removed.is_none() {
        bail!("Profile '{name}' was not found.");
    }
    if config.default_profile.as_deref() == Some(name) {
        config.default_profile = None;
    }
    write_config(&config_path, &config)
}

pub fn set_default_profile(name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let config_path = get_config_path()?;
    let mut config = get_config()?;
    if name != DEFAULT_PROFILE
        && !config
            .profiles
            .as_ref()
            .is_some_and(|profiles| profiles.contains_key(name))
    {
        bail!("Profile '{name}' was not found.");
    }
    config.default_profile = (name != DEFAULT_PROFILE).then(|| name.to_owned());
    write_config(&config_path, &config)
}

/// Load a repository-local profile from `.stashbase/agents/<name>.toml`. These
/// files never create config directories and are ignored when absent.
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

/// List every repository-local profile from `.stashbase/agents`.
pub fn get_directory_agent_profiles() -> Result<Vec<(String, LoadedDirectoryAgentProfile)>> {
    let current_dir = std::env::current_dir()?;
    get_directory_agent_profiles_from_dir(&current_dir)
}

fn get_directory_agent_profiles_from_dir(
    directory: &Path,
) -> Result<Vec<(String, LoadedDirectoryAgentProfile)>> {
    ensure_removed_directory_profile_file_is_absent(directory)?;
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
    ensure_removed_directory_profile_file_is_absent(directory)?;

    let modern_path = directory
        .join(DIRECTORY_AGENT_PROFILES_DIR)
        .join(format!("{profile_name}.toml"));
    if !modern_path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&modern_path).with_context(|| {
        format!(
            "Could not read agent profile file '{}'.",
            modern_path.display()
        )
    })?;
    let profile = toml::from_str::<AgentProfile>(&content).with_context(|| {
        format!(
            "Could not parse agent profile file '{}'.",
            modern_path.display()
        )
    })?;
    Ok(Some(LoadedDirectoryAgentProfile {
        profile,
        source: display_directory_profile_path(directory, &modern_path),
        path: modern_path,
    }))
}

fn ensure_removed_directory_profile_file_is_absent(directory: &Path) -> Result<()> {
    let removed_path = directory.join(REMOVED_DIRECTORY_AGENT_PROFILE_FILE);
    if removed_path.is_file() {
        bail!("Repository-local agent profiles must use '.stashbase/agents/<profile>.toml'.");
    }
    Ok(())
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
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        get_directory_agent_profile_from_dir, get_directory_agent_profiles_from_dir,
        get_explicit_agent_profile, resolve_profile_name, DIRECTORY_AGENT_PROFILES_DIR,
    };
    use crate::models::config::{Config, ProfileConfig};

    fn temporary_directory() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "stashbase-agent-profile-test-{}-{unique}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
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
    fn rejects_removed_single_file_directory_profile_layout() {
        let directory = temporary_directory();
        fs::write(
            directory.join("stashbase-agent.toml"),
            "[agent_profiles.codex]\negress_hosts = [\"api.github.com\"]\n",
        )
        .unwrap();

        let error = get_directory_agent_profile_from_dir(&directory, "codex")
            .unwrap_err()
            .to_string();
        assert!(error.contains(".stashbase/agents/<profile>.toml"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn lists_direct_directory_profiles() {
        let directory = temporary_directory();
        let agents = directory.join(DIRECTORY_AGENT_PROFILES_DIR);
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("codex.toml"),
            "egress_hosts = [\"api.github.com\"]\n",
        )
        .unwrap();
        let profiles = get_directory_agent_profiles_from_dir(&directory).unwrap();
        assert_eq!(
            profiles
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["codex"]
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

    #[test]
    fn resolves_explicit_profile_before_config_default() {
        let mut config = Config::new();
        config.default_profile = Some("acme".to_owned());
        config.profiles = Some(std::collections::BTreeMap::from([
            ("acme".to_owned(), ProfileConfig::default()),
            ("personal".to_owned(), ProfileConfig::default()),
        ]));

        assert_eq!(
            resolve_profile_name(&config, Some("personal")).unwrap(),
            "personal"
        );
    }

    #[test]
    fn rejects_unknown_named_profile() {
        let error = resolve_profile_name(&Config::new(), Some("missing"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Profile 'missing' was not found"));
    }
}
