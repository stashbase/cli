use anyhow::{bail, Context, Result};
use log::debug;
use std::{fs, path::Path};

use colored_json::to_colored_json_auto;
use owo_colors::OwoColorize;

use crate::{cmd::secrets::SecretsFromat, models::secrets::Secret};

pub fn format_secrets(secrets: Vec<Secret>, format: &SecretsFromat) -> String {
    match format {
        SecretsFromat::List => {
            let mut text_to_print = String::new();

            for (i, p) in secrets.iter().enumerate() {
                // is last
                if i == secrets.len() - 1 {
                    if p.description.is_some() {
                        text_to_print.push_str(&format!("\n{}", p))
                    } else {
                        text_to_print.push_str(&format!("{}", p))
                    }
                } else {
                    if i != 0 && p.description.is_some() {
                        text_to_print.push_str(&format!("\n{}\n", p))
                    } else {
                        text_to_print.push_str(&format!("{}\n", p))
                    }
                }
            }

            text_to_print
        }
        SecretsFromat::Dotenv => {
            let dotenv_string: String = secrets
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    if let Some(descr) = &s.description {
                        if i != secrets.len() - 1 {
                            return format!("# {}\n{}={}\n", descr, s.key, s.value);
                        } else {
                            return format!("# {}\n{}={}", descr, s.key, s.value);
                        }
                    } else {
                        if i != secrets.len() - 1 {
                            return format!("{}={}\n", s.key, s.value);
                        } else {
                            return format!("{}={}", s.key, s.value);
                        }
                    }
                })
                .collect::<_>();

            dotenv_string
        }
        SecretsFromat::Json => {
            let value = serde_json::to_value(&secrets).unwrap();
            let pretty = to_colored_json_auto(&value).unwrap();

            pretty
        }
    }
}

pub fn format_secret_keys(keys: Vec<String>, format: &SecretsFromat) -> String {
    match format {
        SecretsFromat::List => {
            let mut text_to_print = String::new();

            for (i, p) in keys.iter().enumerate() {
                // is last
                if i == keys.len() - 1 {
                    text_to_print.push_str(&format!("{}", p.green()))
                } else {
                    text_to_print.push_str(&format!("{}\n", p.green()))
                }
            }

            text_to_print
        }
        SecretsFromat::Dotenv => {
            let mut text_to_print = String::new();

            for (i, p) in keys.iter().enumerate() {
                // is last
                if i == keys.len() - 1 {
                    text_to_print.push_str(&format!("{}", p))
                } else {
                    text_to_print.push_str(&format!("{}\n", p))
                }
            }

            text_to_print
        }
        SecretsFromat::Json => {
            let value = serde_json::to_value(&keys).unwrap();
            let pretty = to_colored_json_auto(&value).unwrap();

            pretty
        }
    }
}

// file must exist
pub fn read_dotenv_file(path: &Path) -> Result<Vec<Secret>> {
    let content = fs::read_to_string(&path).context("Failed to read selcted file".red())?;
    let file_is_empty = content.trim().is_empty();

    if file_is_empty {
        bail!("file is empty");
    }

    let splitted: Vec<&str> = content.split("\n").collect();

    if splitted.is_empty() {
        bail!("no secrets found");
    }

    let mut secrets: Vec<Secret> = Vec::new();

    // TODO: format
    for item in splitted {
        let trimmed = item.trim();

        debug!("{}", trimmed);
        debug!("{}", trimmed.len());

        let is_empty = trimmed.len() == 0;
        let is_comment = trimmed.starts_with("#");

        // TODO: accepts comments
        if !is_empty && !is_comment {
            match item.split_once("=") {
                Some((key, value)) => {
                    debug!("{}", key);
                    debug!("{}", value);

                    let secret = Secret {
                        description: None,
                        key: format!("{}", key),
                        value: format!("{}", value),
                    };

                    secrets.push(secret);
                }
                None => {
                    // TODO: accept key with empty value
                    panic!();
                }
            }
        }
    }

    if secrets.is_empty() {
        bail!("no secrets found");
    }

    debug!("{:#?}", secrets);

    Ok(secrets)

    // NOTE: js version
    // if (!line.startsWith('#') && line?.trim() !== '') {
    //         const [key, value] = line.split('=')
    //         // envArray.push({ key, value: value || '' }) // Use an empty string if there's no value
    //
    //         const formattedKey = key
    //           .replace(/[^a-zA-Z0-9 ]/g, '_')
    //           .replace(/ /g, '_')
    //           .toUpperCase()
    //
    //         envArray.push({ key: formattedKey, value: value || '' }) // Use an empty string if there's no value
    //       }
}
