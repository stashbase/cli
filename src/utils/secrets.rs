use anyhow::{bail, Context, Result};
use log::debug;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use colored_json::to_colored_json_auto;
use owo_colors::OwoColorize;

use crate::{
    cmd::{config::SecretsOutputFormat, secrets::SecretsFileFormat},
    models::secrets::{Secret, SecretOnlyName, SecretWithDescription, SecretWithoutDescription},
};

use super::tables::build::build_table;

pub fn format_secrets(secrets: Vec<Secret>, format: &SecretsOutputFormat) -> String {
    match format {
        SecretsOutputFormat::List => {
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
        SecretsOutputFormat::Json => {
            let value = serde_json::to_value(&secrets).unwrap();
            let pretty = to_colored_json_auto(&value).unwrap();

            pretty
        }
        SecretsOutputFormat::Table => {
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
        _ => {
            let kv_separator = match format {
                SecretsOutputFormat::Dotenv => "=",
                SecretsOutputFormat::Yaml => ":",
                _ => unreachable!(),
            };

            // NOTE: YAML or dotenv
            let output_string: String = secrets
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let is_last = i == secrets.len() - 1;
                    let is_multiline = s.value.contains("\n");

                    let replaced_value = match SecretsOutputFormat::Dotenv == *format {
                        true => format!("\"{}\"", s.value.replace("\"", "\\\"")),
                        false => s.value.replace("\"", "\\\""),
                    };

                    if let Some(descr) = &s.description {
                        // let mut str_line =
                        //     format!("# {}\n{}{}{}", descr, s.name, kv_separator, s.value);

                        let mut str_line = match is_multiline {
                            true => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!(
                                        "# {}\n{}{} {}",
                                        descr, s.name, kv_separator, replaced_value
                                    )
                                } else {
                                    let indented_value = replaced_value
                                        .lines()
                                        .map(|line| format!("  {}", line))
                                        .collect::<Vec<_>>()
                                        .join("\n");

                                    format!(
                                        "# {}\n{}{} |\n{}",
                                        descr, s.name, kv_separator, indented_value
                                    )
                                }
                            }
                            false => {
                                format!(
                                    "# {}\n{}{} {}",
                                    descr, s.name, kv_separator, replaced_value
                                )
                            }
                        };

                        // let mut str_line = format!(
                        //     "# {}\n{}{}{}",
                        //     descr,
                        //     s.name,
                        //     kv_separator,
                        //     format!("\"{}\"", s.value.replace("\"", "\\\""))
                        // );

                        if is_last == false {
                            str_line = format!("{}\n", str_line);
                        }

                        if i != 0 {
                            str_line = format!("\n{}", str_line);
                        }

                        return str_line;
                    } else {
                        let prev_has_description = match i == 0 {
                            true => false,
                            false => {
                                let prev_line = secrets.get(i - 1);
                                match prev_line {
                                    Some(prev_line) => prev_line.description.is_some(),
                                    None => false,
                                }
                            }
                        };

                        let is_multiline = s.value.contains("\n");

                        let prev_is_multiline = match i == 0 {
                            true => false,
                            false => {
                                let prev_line = secrets.get(i - 1);
                                match prev_line {
                                    Some(prev_line) => prev_line.value.contains("\n"),
                                    None => false,
                                }
                            }
                        };

                        let mut str_line = match is_multiline {
                            true => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!("{}{} {}", s.name, kv_separator, replaced_value)
                                } else {
                                    let indented_value = replaced_value
                                        .lines()
                                        .map(|line| format!("  {}", line))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    format!("{}{} |\n{}", s.name, kv_separator, indented_value)
                                }
                            }
                            false => {
                                format!("{}{} {}", s.name, kv_separator, replaced_value)
                            }
                        };

                        if i != 0 {
                            if prev_has_description || is_multiline || prev_is_multiline {
                                str_line = format!("\n{}", str_line);
                            }
                        }

                        if is_last == false {
                            str_line = format!("{}\n", str_line);
                        }

                        return str_line;
                    }
                })
                .collect::<_>();

            output_string
        }
    }
}

pub fn format_secret_names(names: Vec<String>, format: &SecretsOutputFormat) -> String {
    match format {
        SecretsOutputFormat::List => {
            let mut text_to_print = String::new();

            for (i, p) in names.iter().enumerate() {
                // is last
                if i == names.len() - 1 {
                    text_to_print.push_str(&format!("{}", p.green()))
                } else {
                    text_to_print.push_str(&format!("{}\n", p.green()))
                }
            }

            text_to_print
        }
        SecretsOutputFormat::Dotenv => {
            let mut text_to_print = String::new();

            for (i, p) in names.iter().enumerate() {
                // is last
                if i == names.len() - 1 {
                    text_to_print.push_str(&format!("{}", p))
                } else {
                    text_to_print.push_str(&format!("{}\n", p))
                }
            }

            text_to_print
        }
        SecretsOutputFormat::Yaml => {
            let mut text_to_print = String::new();

            for (i, p) in names.iter().enumerate() {
                // is last
                if i == names.len() - 1 {
                    text_to_print.push_str(&format!("{}", p))
                } else {
                    text_to_print.push_str(&format!("{}\n", p))
                }
            }

            text_to_print
        }
        SecretsOutputFormat::Json => {
            let value = serde_json::to_value(&names).unwrap();
            let pretty = to_colored_json_auto(&value).unwrap();

            pretty
        }
        SecretsOutputFormat::Table => {
            let table_secrets = names
                .into_iter()
                .map(|s| {
                    let secret: SecretOnlyName = s.into();
                    secret
                })
                .collect::<Vec<_>>();

            build_table(&table_secrets).to_string()
        }
    }
}

pub fn read_secrets_from_file(path: &Path, format: &SecretsFileFormat) -> Result<Vec<Secret>> {
    let content = fs::read_to_string(&path)?;
    let file_is_empty = content.trim().is_empty();

    if file_is_empty {
        return Ok(vec![]);
    }

    match format {
        SecretsFileFormat::Yaml => parse_yaml_secrets_from_str(&content),
        SecretsFileFormat::Dotenv => parse_dotenv_secrets_from_str(&content),
        SecretsFileFormat::Json => {
            let value = serde_json::from_str(&content)?;
            Ok(value)
        }
    }
}

pub fn parse_dotenv_secrets_from_str(content: &String) -> Result<Vec<Secret>> {
    let lines: Vec<&str> = content.trim().split('\n').collect();
    let mut secrets: Vec<Secret> = Vec::new();
    let regex = Regex::new(r"[^A-Z0-9]+").unwrap();

    let mut current_multiline_value: Vec<String> = Vec::new();
    let mut is_in_multiline = false;
    let mut pending_secret: Option<(String, Option<String>)> = None; // (name, description)

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.starts_with('#') {
            continue;
        }

        // Handle multiline mode
        if is_in_multiline {
            current_multiline_value.push(line.to_string());

            if trimmed.ends_with("\"") {
                is_in_multiline = false;
                if let Some((name, description)) = pending_secret.take() {
                    // Join all lines and handle the case where the quote is at the end of first line
                    let full_value = current_multiline_value
                        .iter()
                        .enumerate()
                        .map(|(idx, line)| {
                            if idx == 0 && line.trim() == "\"" {
                                "\"".to_string() // Keep the opening quote if it's alone
                            } else {
                                line.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Remove surrounding quotes and handle escaping
                    let clean_value = if full_value.starts_with("\"") && full_value.ends_with("\"")
                    {
                        full_value[1..full_value.len() - 1].to_string()
                    } else {
                        full_value
                    };

                    secrets.push(Secret {
                        name,
                        value: clean_value,
                        description,
                    });
                    current_multiline_value.clear();
                }
            }
            continue;
        }

        // Process new secrets only when not in multiline mode
        if !trimmed.is_empty() {
            if let Some((name_part, value_part)) = line.split_once('=') {
                let formatted_name = regex
                    .replace_all(&name_part.to_uppercase(), "_")
                    .trim()
                    .to_owned();

                // Get description from previous line if it's a comment
                let description = if index > 0 {
                    let prev_line = lines[index - 1].trim();
                    if prev_line.starts_with('#') {
                        Some(prev_line.replace('#', "").trim().to_owned())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let trimmed_value = value_part.trim();

                // Check if this starts a multiline value
                if trimmed_value.starts_with("\"") && !trimmed_value.ends_with("\"") {
                    is_in_multiline = true;
                    current_multiline_value = vec![value_part.to_string()];
                    pending_secret = Some((formatted_name, description));
                } else if trimmed_value == "\"" {
                    // Handle case where value is just a quote
                    is_in_multiline = true;
                    current_multiline_value = vec![value_part.to_string()];
                    pending_secret = Some((formatted_name, description));
                } else {
                    // Single line value
                    let clean_value =
                        if trimmed_value.starts_with("\"") && trimmed_value.ends_with("\"") {
                            trimmed_value[1..trimmed_value.len() - 1].to_string()
                        } else {
                            trimmed_value.to_string()
                        };

                    secrets.push(Secret {
                        name: formatted_name,
                        value: clean_value,
                        description,
                    });
                }
            }
        }
    }

    // Handle any remaining multiline value
    if is_in_multiline {
        if let Some((name, description)) = pending_secret {
            let full_value = current_multiline_value.join("\n");
            let clean_value = if full_value.starts_with("\"") && full_value.ends_with("\"") {
                full_value[1..full_value.len() - 1].to_string()
            } else {
                full_value
            };

            secrets.push(Secret {
                name,
                value: clean_value,
                description,
            });
        }
    }

    Ok(secrets)
}

pub fn parse_yaml_secrets_from_str(content: &String) -> Result<Vec<Secret>> {
    let lines: Vec<&str> = content.trim().split('\n').collect();
    let mut secrets: Vec<Secret> = Vec::new();
    let regex = Regex::new(r"[^A-Z0-9]+").unwrap();

    let mut current_multiline_value: Vec<String> = Vec::new();
    let mut is_in_multiline = false;
    let mut pending_secret: Option<(String, Option<String>)> = None;
    let mut last_indent: Option<usize> = None;

    for (index, line) in lines.iter().enumerate() {
        // Count leading spaces for indentation
        let leading_spaces = line.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if is_in_multiline {
            // Check if we're still in the indented block by comparing indentation
            if let Some(indent) = last_indent {
                if leading_spaces >= indent {
                    // Still in multiline block, collect the line
                    current_multiline_value.push(trimmed.to_string());
                    continue;
                } else {
                    // Indentation decreased, end of multiline block
                    is_in_multiline = false;
                    if let Some((name, description)) = pending_secret.take() {
                        secrets.push(Secret {
                            name,
                            value: current_multiline_value.join("\n"),
                            description,
                        });
                    }
                    current_multiline_value.clear();
                    last_indent = None;
                }
            }
        }

        // Process new secret
        if let Some((name_part, value_part)) = line.split_once(':') {
            let formatted_name = regex
                .replace_all(&name_part.trim().to_uppercase(), "_")
                .trim()
                .to_owned();

            // Get description from previous line if it's a comment
            let description = if index > 0 {
                let prev_line = lines[index - 1].trim();
                if prev_line.starts_with('#') {
                    Some(prev_line.replace('#', "").trim().to_owned())
                } else {
                    None
                }
            } else {
                None
            };

            let trimmed_value = value_part.trim();

            // Check if this starts a multiline value
            if trimmed_value == "|" {
                is_in_multiline = true;
                current_multiline_value.clear();
                pending_secret = Some((formatted_name, description));

                // Store the indentation level for the multiline block
                if index + 1 < lines.len() {
                    let next_line = lines[index + 1];
                    last_indent = Some(next_line.chars().take_while(|c| c.is_whitespace()).count());
                }
            } else {
                // Single line value
                secrets.push(Secret {
                    name: formatted_name,
                    value: trimmed_value.to_owned(),
                    description,
                });
            }
        }
    }

    // Handle any remaining multiline value at the end of file
    if is_in_multiline && !current_multiline_value.is_empty() {
        if let Some((name, description)) = pending_secret {
            secrets.push(Secret {
                name,
                value: current_multiline_value.join("\n"),
                description,
            });
        }
    }

    Ok(secrets)
}

// file must exist
pub fn parse_secrets_from_str(content: &String, is_yaml: bool) -> Result<Vec<Secret>> {
    let splitted: Vec<&str> = content.split("\n").collect();

    if splitted.is_empty() {
        return Ok(vec![]);
    }

    let mut secrets: Vec<Secret> = Vec::new();
    let regex = Regex::new(r"[^A-Z0-9]+").unwrap();

    let delimiter = match is_yaml {
        true => ":",
        false => "=",
    };

    for (index, item) in splitted.iter().enumerate() {
        let trimmed = item.trim();

        debug!("{}", trimmed);
        debug!("{}", trimmed.len());

        let is_empty = trimmed.len() == 0;
        let is_comment = trimmed.starts_with("#");

        if !is_empty && !is_comment {
            match item.split_once(delimiter) {
                Some((name, value)) => {
                    debug!("{}", name);
                    debug!("{}", value);

                    let uppercase_name = name.to_uppercase();
                    let formatted_name = regex.replace_all(&uppercase_name, "_").trim().to_owned();
                    let formatted_value = value.trim().to_owned();

                    let description = match index == 0 {
                        true => None,
                        false => {
                            let prev_line = splitted.get(index - 1);
                            match prev_line {
                                Some(prev_line) => match prev_line.trim().starts_with("#") {
                                    true => {
                                        // replace all
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
                        name: format!("{}", formatted_name),
                        value: format!("{}", formatted_value),
                    };

                    secrets.push(secret);
                }
                None => {
                    // NOTE: do nothing

                    // let secret = Secret {
                    //     description,
                    //     key: format!("{}", trimmed),
                    //     value: format!(""),
                    // };
                    //
                    // secrets.push(secret);
                }
            }
        }
    }

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

pub fn find_duplicate_names(array: &[Secret]) -> Vec<String> {
    let mut name_count = HashMap::new();

    // Count occurrences of each name
    for item in array {
        *name_count.entry(&item.name).or_insert(0) += 1;
    }

    // Collect names with more than one occurrence
    name_count
        .into_iter()
        .filter_map(|(name, count)| if count > 1 { Some(name.clone()) } else { None })
        .collect()
}

pub fn extract_unique_references_from_secret(secret_value: &str) -> HashSet<String> {
    // Define the regular expression to match ${...}
    let regex = Regex::new(r"\$\{(.*?)\}").unwrap();
    // Create a HashSet to store unique references
    let mut refs = HashSet::new();

    // Iterate over all matches
    for cap in regex.captures_iter(secret_value) {
        // cap[1] contains the captured group inside ${}
        refs.insert(cap[1].to_string());
    }

    return refs;
    // // Convert the HashSet back to a Vec
    // let unique_refs: Vec<String> = refs.into_iter().collect();
    //
    // unique_refs
}

pub fn expand_secret_references(secrets: &mut Vec<Secret>) {
    let secrets_map: HashMap<_, _> = secrets
        .iter()
        .map(|s| (s.name.clone(), s.value.clone()))
        .collect();

    for secret in secrets {
        let all_unique_refs = extract_unique_references_from_secret(&secret.value);

        if !all_unique_refs.is_empty() {
            for ref_ in all_unique_refs {
                if let Some(value) = secrets_map.get(&ref_) {
                    let to_replace = format!("${{{}}}", ref_);
                    secret.value = secret.value.replace(&to_replace, value);
                }
            }
        }
    }
}
