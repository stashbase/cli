use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use log::debug;

use crate::models::config::{Config, OutputFormatConfig, UpdateConfig};

pub fn create_config(path: &Path) -> Result<String> {
    let new_config = Config::new();

    let toml_string = toml::to_string(&new_config)
        .with_context(|| format!("Could not create toml config file"))?;
    debug!("new toml string: {}", &toml_string);

    fs::write(path, &toml_string)?;

    Ok(toml_string)
}

pub fn get_config_path() -> Result<PathBuf> {
    let dir_path = ProjectDirs::from("app", "env-ease", "env-ease.toml");

    match dir_path {
        Some(dirs) => Ok(dirs.config_dir().to_path_buf()),
        None => bail!("Could not find config directory"),
    }
}

pub fn get_config() -> Result<Config> {
    if let Some(proj_dirs) = ProjectDirs::from("app", "env-ease", "env-ease.toml") {
        let config_dir = proj_dirs.config_dir();
        let config_file_exists = Path::new(&config_dir).is_file();

        match config_file_exists {
            true => {
                let content = fs::read_to_string(config_dir)?;
                let data =
                    toml::from_str::<Config>(&content).context("Could not parse config file")?;

                Ok(data)
            }
            false => {
                let new_config = create_config(config_dir)?;
                let data = toml::from_str::<Config>(&new_config)?;

                Ok(data)
            }
        }
    } else {
        bail!("Could not find config directory")
    }
}

pub fn update_config(args: UpdateConfig) -> Result<()> {
    let config_path = get_config_path()?;
    let mut config = get_config()?;

    let UpdateConfig {
        api_key,
        output_format,
    } = args;

    if let Some(new_api_key) = api_key {
        config.api_key = Some(new_api_key);
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

    let config_string = toml::to_string(&config)?;
    fs::write(config_path, config_string)?;

    Ok(())
}
