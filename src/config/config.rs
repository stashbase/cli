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

/// Load an optional complete agent profile from `.stashbase.toml` in the current
/// directory. This file never creates config directories and is ignored when absent.
pub fn get_directory_agent_profile(profile_name: &str) -> Result<Option<AgentProfile>> {
    let path = std::env::current_dir()?.join(".stashbase.toml");
    if !path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Could not read project config file '{}'.", path.display()))?;
    let config: DirectoryConfig = toml::from_str(&content)
        .with_context(|| format!("Could not parse project config file '{}'.", path.display()))?;
    Ok(config
        .agent_profiles
        .and_then(|profiles| profiles.get(profile_name).cloned()))
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
