use anyhow::{bail, Context, Result};
use log::debug;
use regex::Regex;
use std::{fs, path::Path};

use colored_json::to_colored_json_auto;
use owo_colors::OwoColorize;

use crate::{
    cmd::secrets::SecretsFromat,
    models::secrets::{Secret, SecretOnlyKey, SecretWithDescription, SecretWithoutDescription},
};

use super::tables::build::build_table;

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
        SecretsFromat::Table => {
            // TODO: cehck no have description -> dont show descr col
            let has_some_description = secrets.iter().any(|s| s.has_description());

            if has_some_description {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| {
                        let secret: SecretWithDescription = s.into();
                        secret
                    })
                    .collect::<Vec<_>>();

                build_table(&table_secrets).to_string()
            } else {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| {
                        let secret: SecretWithoutDescription = s.into();
                        secret
                    })
                    .collect::<Vec<_>>();

                build_table(&table_secrets).to_string()
            }
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
        SecretsFromat::Table => {
            let table_secrets = keys
                .into_iter()
                .map(|s| {
                    let secret: SecretOnlyKey = s.into();
                    secret
                })
                .collect::<Vec<_>>();

            build_table(&table_secrets).to_string()
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
    let regex = Regex::new(r"[^A-Z0-9]+").unwrap();

    // TODO: format
    for (index, item) in splitted.iter().enumerate() {
        let trimmed = item.trim();

        debug!("{}", trimmed);
        debug!("{}", trimmed.len());

        let is_empty = trimmed.len() == 0;
        let is_comment = trimmed.starts_with("#");

        if !is_empty && !is_comment {
            match item.split_once("=") {
                Some((key, value)) => {
                    debug!("{}", key);
                    debug!("{}", value);

                    let uppercase_key = key.to_uppercase();
                    let formatted_key = regex.replace_all(&uppercase_key, "_").to_string();

                    let description = match index == 0 {
                        true => None,
                        false => {
                            let prev_line = splitted.get(index - 1);
                            match prev_line {
                                Some(prev_line) => match prev_line.trim().starts_with("#") {
                                    true => {
                                        let d = prev_line.replace("#", "").trim().to_owned();
                                        Some(d)
                                    }
                                    false => None,
                                },
                                None => None,
                            }
                        }
                    };

                    let secret = Secret {
                        description,
                        key: format!("{}", formatted_key),
                        value: format!("{}", value),
                    };

                    secrets.push(secret);
                }
                None => {
                    // TODO: accept key with empty value or error
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
