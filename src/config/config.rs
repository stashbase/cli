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
/// Global profiles: `agents/<name>.toml` next to `config.toml`, the same
/// one-file-per-profile format as directory profiles.
pub const GLOBAL_AGENT_PROFILES_DIR: &str = "agents";

#[derive(Debug, Clone)]
pub struct LoadedAgentProfile {
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

pub fn resolve_profile_name(config: &Config) -> Result<String> {
    let profile = get_env_var(PROFILE_ENV_VAR)
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
pub fn get_directory_agent_profile(profile_name: &str) -> Result<Option<LoadedAgentProfile>> {
    let current_dir = std::env::current_dir()?;
    get_directory_agent_profile_from_dir(&current_dir, profile_name)
}

/// Load one direct profile file for explicit CI or automation use.
pub fn get_explicit_agent_profile(path: &Path) -> Result<LoadedAgentProfile> {
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
    Ok(LoadedAgentProfile {
        profile,
        source: path.display().to_string(),
        path,
    })
}

/// List every repository-local profile from `.stashbase/agents`.
pub fn get_directory_agent_profiles() -> Result<Vec<(String, LoadedAgentProfile)>> {
    let current_dir = std::env::current_dir()?;
    get_directory_agent_profiles_from_dir(&current_dir)
}

fn get_directory_agent_profiles_from_dir(
    directory: &Path,
) -> Result<Vec<(String, LoadedAgentProfile)>> {
    ensure_removed_directory_profile_file_is_absent(directory)?;
    load_agent_profiles_in(&directory.join(DIRECTORY_AGENT_PROFILES_DIR), |path| {
        display_directory_profile_path(directory, path)
    })
}

fn get_directory_agent_profile_from_dir(
    directory: &Path,
    profile_name: &str,
) -> Result<Option<LoadedAgentProfile>> {
    if !is_safe_profile_file_name(profile_name) {
        bail!(
            "Agent profile name '{profile_name}' must be a plain file name when using directory profiles."
        );
    }
    ensure_removed_directory_profile_file_is_absent(directory)?;
    load_agent_profile_in(
        &directory.join(DIRECTORY_AGENT_PROFILES_DIR),
        profile_name,
        |path| display_directory_profile_path(directory, path),
    )
}

/// The directory holding global profiles. Not created here: a missing
/// directory just means there are no global profiles.
pub fn global_agent_profiles_dir() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("", "", "stashbase").context("Could not find config directory.")?;
    Ok(dirs.config_dir().join(GLOBAL_AGENT_PROFILES_DIR))
}

/// Load a global profile from `agents/<name>.toml` next to `config.toml`.
pub fn get_global_agent_profile(profile_name: &str) -> Result<Option<LoadedAgentProfile>> {
    if !is_safe_profile_file_name(profile_name) {
        bail!("Agent profile name '{profile_name}' must be a plain file name.");
    }
    let agents_directory = global_agent_profiles_dir()?;
    ensure_removed_global_profiles_are_absent(&agents_directory)?;
    load_agent_profile_in(&agents_directory, profile_name, display_global_profile_path)
}

/// List every global profile.
pub fn get_global_agent_profiles() -> Result<Vec<(String, LoadedAgentProfile)>> {
    let agents_directory = global_agent_profiles_dir()?;
    ensure_removed_global_profiles_are_absent(&agents_directory)?;
    load_agent_profiles_in(&agents_directory, display_global_profile_path)
}

/// Global profiles used to be `[agent_profiles.<name>]` tables in
/// `config.toml`. They are no longer read; rather than ignore them silently,
/// say where they go now.
fn ensure_removed_global_profiles_are_absent(agents_directory: &Path) -> Result<()> {
    let config = get_config()?;
    match removed_global_profiles_error(&config, agents_directory, &get_config_path()?) {
        Some(error) => bail!(error),
        None => Ok(()),
    }
}

fn removed_global_profiles_error(
    config: &Config,
    agents_directory: &Path,
    config_path: &Path,
) -> Option<String> {
    let names = config
        .removed_agent_profiles
        .as_ref()
        .map(|table| table.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "Global agent profiles are no longer read from config.toml. Move each [agent_profiles.<name>] table ({}) to its own file, {}/<name>.toml, without the 'agent_profiles.<name>' prefix, then remove it from {}.",
        names.join(", "),
        display_global_path(agents_directory),
        display_global_path(config_path)
    ))
}

fn load_agent_profiles_in(
    agents_directory: &Path,
    display: impl Fn(&Path) -> String,
) -> Result<Vec<(String, LoadedAgentProfile)>> {
    let mut profiles = std::collections::BTreeMap::new();
    if agents_directory.is_dir() {
        for entry in fs::read_dir(agents_directory).with_context(|| {
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
            let profile = read_agent_profile_file(&path)?;
            profiles.insert(
                name.to_owned(),
                LoadedAgentProfile {
                    profile,
                    source: display(&path),
                    path,
                },
            );
        }
    }
    Ok(profiles.into_iter().collect())
}

fn load_agent_profile_in(
    agents_directory: &Path,
    profile_name: &str,
    display: impl Fn(&Path) -> String,
) -> Result<Option<LoadedAgentProfile>> {
    let path = agents_directory.join(format!("{profile_name}.toml"));
    if !path.is_file() {
        return Ok(None);
    }
    let profile = read_agent_profile_file(&path)?;
    Ok(Some(LoadedAgentProfile {
        profile,
        source: display(&path),
        path,
    }))
}

fn read_agent_profile_file(path: &Path) -> Result<AgentProfile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Could not read agent profile file '{}'.", path.display()))?;
    toml::from_str::<AgentProfile>(&content)
        .with_context(|| format!("Could not parse agent profile file '{}'.", path.display()))
}

/// A global path for messages, with the home directory shown as `~`.
pub fn display_global_path(path: &Path) -> String {
    directories::BaseDirs::new()
        .and_then(|dirs| {
            path.strip_prefix(dirs.home_dir())
                .ok()
                .map(|relative| format!("~/{}", relative.display()))
        })
        .unwrap_or_else(|| path.display().to_string())
}

fn display_global_profile_path(path: &Path) -> String {
    display_global_path(path)
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
        display_global_path, get_directory_agent_profile_from_dir,
        get_directory_agent_profiles_from_dir, get_explicit_agent_profile, load_agent_profile_in,
        load_agent_profiles_in, removed_global_profiles_error, resolve_profile_name,
        DEFAULT_PROFILE, DIRECTORY_AGENT_PROFILES_DIR,
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
        // Built the same way production does (join + `Display`) rather
        // than a hardcoded forward-slash literal: `Path`'s `Display` uses
        // the OS-native separator, so a literal like this only matches on
        // Unix — on Windows the real value has backslashes in the nested
        // part (`./.stashbase/agents\codex.toml`), which isn't a bug in
        // the production code, just something a hardcoded literal can't
        // account for.
        let expected_source = format!(
            "./{}",
            std::path::PathBuf::from(DIRECTORY_AGENT_PROFILES_DIR)
                .join("codex.toml")
                .display()
        );
        assert_eq!(loaded.source, expected_source);
        assert_eq!(
            loaded.profile.secrets.bindings["GITHUB_TOKEN"].hosts,
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
    fn loads_and_lists_profile_files_from_a_global_agents_directory() {
        let directory = temporary_directory();
        let agents = directory.join("agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("multi-repo.toml"),
            "egress_hosts = [\"api.anthropic.com\"]\n[workspace]\nextra_paths = [\"~/Projects/cli\"]\n",
        )
        .unwrap();
        fs::write(agents.join("notes.txt"), "ignored").unwrap();

        let loaded = load_agent_profile_in(&agents, "multi-repo", display_global_path)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.path, agents.join("multi-repo.toml"));
        assert_eq!(loaded.source, display_global_path(&loaded.path));
        assert_eq!(loaded.profile.workspace.extra_paths.len(), 1);
        assert!(
            load_agent_profile_in(&agents, "missing", display_global_path)
                .unwrap()
                .is_none()
        );

        let names = load_agent_profiles_in(&agents, display_global_path)
            .unwrap()
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["multi-repo"]);
        assert!(
            load_agent_profiles_in(&directory.join("absent"), display_global_path)
                .unwrap()
                .is_empty()
        );

        fs::write(agents.join("broken.toml"), "egress_hosts = 3\n").unwrap();
        let error = format!(
            "{:#}",
            load_agent_profiles_in(&agents, display_global_path).unwrap_err()
        );
        assert!(error.contains("broken.toml"), "{error}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_leftover_agent_profiles_table_points_to_the_new_location() {
        let config: Config =
            toml::from_str("[agent_profiles.coding]\negress_hosts = []\n[agent_profiles.ci]\n")
                .unwrap();
        let agents = std::path::Path::new("/stashbase-config/agents");
        let error =
            removed_global_profiles_error(&config, agents, &agents.with_file_name("config.toml"))
                .unwrap();
        assert!(error.contains("(ci, coding)"), "{error}");
        assert!(
            error.contains("/stashbase-config/agents/<name>.toml"),
            "{error}"
        );
        assert!(removed_global_profiles_error(&Config::new(), agents, agents).is_none());
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
    fn resolves_configured_default_profile() {
        let mut config = Config::new();
        config.default_profile = Some("acme".to_owned());
        config.profiles = Some(std::collections::BTreeMap::from([
            ("acme".to_owned(), ProfileConfig::default()),
            ("personal".to_owned(), ProfileConfig::default()),
        ]));

        assert_eq!(resolve_profile_name(&config).unwrap(), "acme");
    }

    #[test]
    fn rejects_unknown_named_profile() {
        let config = Config::new();
        assert_eq!(resolve_profile_name(&config).unwrap(), DEFAULT_PROFILE);
    }
}
