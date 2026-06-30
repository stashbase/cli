#![allow(dead_code)]

use anyhow::{bail, Result};
use linked_hash_set::LinkedHashSet;
use regex::Regex;
use std::{collections::HashMap, fs, path::Path};

use crate::{
    cmd::{config::SecretsOutputFormat, secrets::SecretsFileFormat},
    models::secrets::{
        Secret, SecretOnlyName, SecretOnlyNameWithComment, SecretOptional, SecretWithComment,
        SecretWithoutComment,
    },
    utils::output::{get_formatted_json_string, ColorizeIfColoredOutput},
};

use super::tables::build::build_table;

pub fn format_secrets(secrets: Vec<Secret>, format: &SecretsOutputFormat) -> String {
    match format {
        SecretsOutputFormat::Plain => {
            let mut text_to_print = String::new();

            for (i, p) in secrets.iter().enumerate() {
                let is_multiline = p.value.contains("\n");
                let prev_is_multiline = i != 0 && secrets[i - 1].value.contains("\n");
                let needs_block_separator = i != 0
                    && (secrets[i - 1].has_comment()
                        || prev_is_multiline
                        || p.comment.is_some()
                        || is_multiline);

                if needs_block_separator {
                    text_to_print.push('\n');
                }

                text_to_print.push_str(&format!("{}", p));

                if i != secrets.len() - 1 {
                    text_to_print.push('\n');
                }
            }

            text_to_print
        }
        SecretsOutputFormat::Json => {
            let pretty = get_formatted_json_string(&secrets, true).unwrap();
            pretty
        }
        SecretsOutputFormat::Table => {
            let has_some_comment = secrets.iter().any(|s| s.has_comment());

            if has_some_comment {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| {
                        let secret: SecretWithComment = s.into();
                        secret
                    })
                    .collect::<Vec<_>>();

                build_table(&table_secrets).to_string()
            } else {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| {
                        let secret: SecretWithoutComment = s.into();
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

                    if let Some(comment) = &s.comment {
                        // let mut str_line =
                        //     format!("# {}\n{}{}{}", descr, s.name, kv_separator, s.value);

                        let comment_str = comment
                            .split('\n')
                            .map(|line| format!("# {}\n", line.trim()))
                            .collect::<String>();

                        let mut str_line = match is_multiline {
                            true => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!(
                                        "{}{}{}{}",
                                        comment_str, s.name, kv_separator, replaced_value
                                    )
                                } else {
                                    let indented_value = replaced_value
                                        .lines()
                                        .map(|line| format!("  {}", line))
                                        .collect::<Vec<_>>()
                                        .join("\n");

                                    format!(
                                        "{}{}{} |\n{}",
                                        comment_str, s.name, kv_separator, indented_value
                                    )
                                }
                            }
                            false => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!(
                                        "{}{}{}{}",
                                        comment_str, s.name, kv_separator, replaced_value
                                    )
                                } else {
                                    format!(
                                        "{}{}{} {}",
                                        comment_str, s.name, kv_separator, replaced_value
                                    )
                                }
                            }
                        };

                        if is_last == false {
                            str_line = format!("{}\n", str_line);
                        }

                        if i != 0 {
                            str_line = format!("\n{}", str_line);
                        }

                        return str_line;
                    } else {
                        let prev_has_comment = match i == 0 {
                            true => false,
                            false => {
                                let prev_line = secrets.get(i - 1);
                                match prev_line {
                                    Some(prev_line) => prev_line.comment.is_some(),
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
                                    format!("{}{}{}", s.name, kv_separator, replaced_value)
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
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!("{}{}{}", s.name, kv_separator, replaced_value)
                                } else {
                                    format!("{}{} {}", s.name, kv_separator, replaced_value)
                                }
                            }
                        };

                        if i != 0 {
                            if prev_has_comment || is_multiline || prev_is_multiline {
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

pub fn format_optional_secrets(
    secrets: Vec<SecretOptional>,
    format: &SecretsOutputFormat,
    hide_missing_values: bool,
) -> String {
    const HIDDEN_VALUE_PLACEHOLDER: &str = "[hidden]";
    let all_without_values = secrets.iter().all(|s| s.value.is_none());

    match format {
        SecretsOutputFormat::Plain => {
            let mut text_to_print = String::new();

            for (i, p) in secrets.iter().enumerate() {
                let value = match (p.value.as_deref(), hide_missing_values) {
                    (Some(value), _) => value,
                    (None, true) => HIDDEN_VALUE_PLACEHOLDER,
                    (None, false) => "",
                };
                let is_multiline = value.contains("\n");
                let prev_value = if i == 0 {
                    ""
                } else {
                    match (secrets[i - 1].value.as_deref(), hide_missing_values) {
                        (Some(value), _) => value,
                        (None, true) => HIDDEN_VALUE_PLACEHOLDER,
                        (None, false) => "",
                    }
                };
                let prev_is_multiline = i != 0 && prev_value.contains("\n");
                let needs_block_separator = i != 0
                    && (secrets[i - 1].has_comment()
                        || prev_is_multiline
                        || p.comment.is_some()
                        || is_multiline);

                if needs_block_separator {
                    text_to_print.push('\n');
                }

                if let Some(comment) = &p.comment {
                    let comment_str = format!("# {}", comment);
                    text_to_print.push_str(&format!("{}\n", comment_str.bright_blue_if_tty()));
                }

                if p.value.is_some() || hide_missing_values {
                    text_to_print.push_str(&format!(
                        "{} {}",
                        format!("{}:", p.name).blue_bold_if_tty(),
                        value
                    ));
                } else {
                    text_to_print.push_str(&p.name.as_str().blue_bold_if_tty().to_string());
                }

                if i != secrets.len() - 1 {
                    text_to_print.push('\n');
                }
            }

            text_to_print
        }
        SecretsOutputFormat::Json => get_formatted_json_string(&secrets, true).unwrap(),
        SecretsOutputFormat::Table => {
            let has_some_comment = secrets.iter().any(|s| s.has_comment());

            if all_without_values && has_some_comment {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| SecretOnlyNameWithComment {
                        name: s.name,
                        comment: s.comment.unwrap_or_default(),
                    })
                    .collect::<Vec<_>>();

                build_table(&table_secrets).to_string()
            } else if has_some_comment {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| SecretWithComment {
                        name: s.name,
                        value: s.value.unwrap_or_default(),
                        comment: s.comment.unwrap_or_default(),
                    })
                    .collect::<Vec<_>>();

                build_table(&table_secrets).to_string()
            } else if all_without_values {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| SecretOnlyName { name: s.name })
                    .collect::<Vec<_>>();

                build_table(&table_secrets).to_string()
            } else {
                let table_secrets = secrets
                    .into_iter()
                    .map(|s| SecretWithoutComment {
                        name: s.name,
                        value: s.value.unwrap_or_default(),
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

            secrets
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let is_last = i == secrets.len() - 1;
                    let value = match (s.value.as_deref(), hide_missing_values) {
                        (Some(value), _) => value,
                        (None, true) => HIDDEN_VALUE_PLACEHOLDER,
                        (None, false) => "",
                    };
                    let is_multiline = value.contains("\n");

                    let replaced_value = match SecretsOutputFormat::Dotenv == *format {
                        true => format!("\"{}\"", value.replace("\"", "\\\"")),
                        false => value.replace("\"", "\\\""),
                    };

                    if let Some(comment) = &s.comment {
                        let comment_str = comment
                            .split('\n')
                            .map(|line| format!("# {}\n", line.trim()))
                            .collect::<String>();

                        let mut str_line = match is_multiline {
                            true => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!(
                                        "{}{}{}{}",
                                        comment_str, s.name, kv_separator, replaced_value
                                    )
                                } else {
                                    let indented_value = replaced_value
                                        .lines()
                                        .map(|line| format!("  {}", line))
                                        .collect::<Vec<_>>()
                                        .join("\n");

                                    format!(
                                        "{}{}{} |\n{}",
                                        comment_str, s.name, kv_separator, indented_value
                                    )
                                }
                            }
                            false => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!(
                                        "{}{}{}{}",
                                        comment_str, s.name, kv_separator, replaced_value
                                    )
                                } else {
                                    format!(
                                        "{}{}{} {}",
                                        comment_str, s.name, kv_separator, replaced_value
                                    )
                                }
                            }
                        };

                        if !is_last {
                            str_line = format!("{}\n", str_line);
                        }

                        if i != 0 {
                            str_line = format!("\n{}", str_line);
                        }

                        str_line
                    } else {
                        let prev_has_comment = match i == 0 {
                            true => false,
                            false => secrets
                                .get(i - 1)
                                .map(|prev_line| prev_line.comment.is_some())
                                .unwrap_or(false),
                        };

                        let prev_is_multiline = match i == 0 {
                            true => false,
                            false => secrets
                                .get(i - 1)
                                .map(|prev_line| {
                                    prev_line
                                        .value
                                        .as_deref()
                                        .unwrap_or_default()
                                        .contains("\n")
                                })
                                .unwrap_or(false),
                        };

                        let mut str_line = match is_multiline {
                            true => {
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!("{}{}{}", s.name, kv_separator, replaced_value)
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
                                if SecretsOutputFormat::Dotenv == *format {
                                    format!("{}{}{}", s.name, kv_separator, replaced_value)
                                } else {
                                    format!("{}{} {}", s.name, kv_separator, replaced_value)
                                }
                            }
                        };

                        if i != 0 && (prev_has_comment || is_multiline || prev_is_multiline) {
                            str_line = format!("\n{}", str_line);
                        }

                        if !is_last {
                            str_line = format!("{}\n", str_line);
                        }

                        str_line
                    }
                })
                .collect::<String>()
        }
    }
}

pub fn format_secret_names(names: Vec<String>, format: &SecretsOutputFormat) -> String {
    match format {
        SecretsOutputFormat::Plain => {
            let mut text_to_print = String::new();

            for (i, p) in names.iter().enumerate() {
                // is last
                if i == names.len() - 1 {
                    text_to_print.push_str(&format!("{}", p.green_if_tty()))
                } else {
                    text_to_print.push_str(&format!("{}\n", p.green_if_tty()))
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
            let pretty = get_formatted_json_string(&names, true).unwrap();
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
    let mut current_multiline_value: Vec<String> = Vec::new();
    let mut is_in_multiline = false;
    let mut pending_secret: Option<(String, Option<String>)> = None;
    let mut comment_lines: Vec<String> = Vec::new();

    for line in lines.iter() {
        let trimmed = line.trim();

        // Handle comments when not in multiline mode
        if !is_in_multiline && trimmed.starts_with('#') {
            if trimmed == "#" {
                comment_lines.push("".to_string());
            } else {
                let mut cleaned_line = line[1..].to_string();
                if cleaned_line.starts_with(' ') {
                    cleaned_line = cleaned_line[1..].trim_end().to_string();
                }
                comment_lines.push(cleaned_line);
            }
            continue;
        }

        // When we hit a non-comment line, trim empty lines from start and end
        if !trimmed.starts_with('#') && !comment_lines.is_empty() {
            // Trim empty lines from start
            while comment_lines.first().map_or(false, |line| line.is_empty()) {
                comment_lines.remove(0);
            }
            // Trim empty lines from end
            while comment_lines.last().map_or(false, |line| line.is_empty()) {
                comment_lines.pop();
            }
        }

        // Handle multiline mode
        if is_in_multiline {
            current_multiline_value.push(line.to_string());

            if ends_with_unescaped_quote(trimmed) {
                // The very first line in multiline mode can be just `"`, which is
                // the opening quote from `KEY="`.
                let is_opening_quote_only_line =
                    trimmed == "\"" && current_multiline_value.len() == 1;

                if !is_opening_quote_only_line {
                    is_in_multiline = false;
                    if let Some((name, comment)) = pending_secret.take() {
                        let full_value = current_multiline_value
                            .iter()
                            .enumerate()
                            .map(|(idx, line)| {
                                if idx == 0 && line.trim() == "\"" {
                                    "\"".to_string()
                                } else {
                                    line.to_string()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        let clean_value = clean_surrounding_quotes(&full_value);
                        let mut unescaped_value = unescape_value_from_dotenv(&clean_value);
                        let has_quote_only_wrapper_lines = current_multiline_value
                            .first()
                            .map(|line| line.trim() == "\"")
                            .unwrap_or(false)
                            && current_multiline_value
                                .last()
                                .map(|line| line.trim() == "\"")
                                .unwrap_or(false);
                        if has_quote_only_wrapper_lines {
                            unescaped_value = trim_outer_newlines(&unescaped_value);
                        }

                        secrets.push(Secret {
                            name,
                            value: unescaped_value,
                            comment,
                        });
                        current_multiline_value.clear();
                    }
                }
            }
            continue;
        }

        // Process new secrets only when not in multiline mode
        if !trimmed.is_empty() {
            if let Some(equal_sign_idx) = line.find('=') {
                let (name_part, value_part) = line.split_at(equal_sign_idx);
                let value_part = &value_part[1..]; // Skip the '=' character

                let name = name_part.trim().to_string();

                // Join multiline comments if they exist
                let comment = if !comment_lines.is_empty() {
                    let desc = comment_lines.join("\n");
                    comment_lines.clear();

                    Some(desc)
                } else {
                    None
                };

                let trimmed_value = value_part.trim();
                if trimmed_value.starts_with("\"") {
                    if !ends_with_unescaped_quote(trimmed_value) || trimmed_value == "\"" {
                        is_in_multiline = true;
                        current_multiline_value = vec![value_part.to_string()];
                        pending_secret = Some((name, comment));
                    } else {
                        // Single line value
                        let cleaned_value = clean_surrounding_quotes(value_part);
                        let unescaped_value = unescape_value_from_dotenv(&cleaned_value);

                        secrets.push(Secret {
                            name,
                            value: unescaped_value,
                            comment,
                        });
                    }
                } else {
                    // Single line value without quotes
                    let cleaned_value = clean_surrounding_quotes(value_part);
                    let unescaped_value = unescape_value_from_dotenv(&cleaned_value);

                    secrets.push(Secret {
                        name,
                        value: unescaped_value,
                        comment,
                    });
                }
            }
        } else {
            // Clear comment lines if we hit an empty line
            comment_lines.clear();
        }
    }

    // Handle any remaining multiline value
    if is_in_multiline {
        if let Some((name, comment)) = pending_secret {
            let full_value = current_multiline_value.join("\n");
            let clean_value = clean_surrounding_quotes(&full_value);
            let mut unescaped_value = unescape_value_from_dotenv(&clean_value);
            let has_quote_only_wrapper_lines = current_multiline_value
                .first()
                .map(|line| line.trim() == "\"")
                .unwrap_or(false)
                && current_multiline_value
                    .last()
                    .map(|line| line.trim() == "\"")
                    .unwrap_or(false);
            if has_quote_only_wrapper_lines {
                unescaped_value = trim_outer_newlines(&unescaped_value);
            }

            secrets.push(Secret {
                name,
                value: unescaped_value,
                comment,
            });
        }
    }

    Ok(secrets)
}

// Helper functions
fn clean_surrounding_quotes(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("\"") && trimmed.ends_with("\"") {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn unescape_value_from_dotenv(value: &str) -> String {
    value
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
}

fn ends_with_unescaped_quote(value: &str) -> bool {
    if !value.ends_with('"') {
        return false;
    }

    // Count consecutive backslashes immediately before the trailing quote.
    // Odd count => quote is escaped, even count => quote is not escaped.
    let mut backslash_count = 0;
    for ch in value[..value.len() - 1].chars().rev() {
        if ch == '\\' {
            backslash_count += 1;
        } else {
            break;
        }
    }

    backslash_count % 2 == 0
}

fn trim_outer_newlines(value: &str) -> String {
    value.trim_matches(|c| c == '\n' || c == '\r').to_string()
}

pub fn parse_yaml_secrets_from_str(content: &String) -> Result<Vec<Secret>> {
    // First pass: collect comments/comments
    let lines: Vec<&str> = content.trim().split('\n').collect();
    let mut comments: HashMap<String, String> = HashMap::new();
    let mut comment_lines: Vec<String> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let leading_spaces = line.chars().take_while(|c| c.is_whitespace()).count();

        if trimmed.starts_with('#') {
            // For comments, preserve indentation after the first space
            let comment_content = if let Some(first_hash) = line.find('#') {
                let after_hash = &line[first_hash + 1..];
                if let Some(first_non_hash) = after_hash.find(|c| c != '#') {
                    let mut cleaned_line = &after_hash[first_non_hash..];
                    if cleaned_line.starts_with(' ') {
                        cleaned_line = cleaned_line[1..].trim_end();
                    }
                    cleaned_line
                } else {
                    ""
                }
            } else {
                ""
            };

            // Only consider comments that are not indented
            if leading_spaces < 2 {
                // Skip empty comments if they would be first or last
                if !comment_content.is_empty()
                    || (!comment_lines.is_empty()
                        && i + 1 < lines.len()
                        && lines[i + 1].trim().starts_with('#'))
                {
                    comment_lines.push(comment_content.to_owned());
                }
            }
            continue;
        }

        if !trimmed.is_empty() && trimmed.contains(':') {
            if let Some(key) = trimmed.split(':').next() {
                if !comment_lines.is_empty() {
                    // Trim empty comments from start and end
                    let mut start = 0;
                    let mut end = comment_lines.len();

                    while start < end && comment_lines[start].trim().is_empty() {
                        start += 1;
                    }
                    while end > start && comment_lines[end - 1].trim().is_empty() {
                        end -= 1;
                    }

                    let filtered_comments = &comment_lines[start..end];
                    if !filtered_comments.is_empty() {
                        comments.insert(key.to_string(), filtered_comments.join("\n"));
                    }
                    comment_lines.clear();
                }
            }
        } else {
            // Clear comment lines if we hit an empty line
            comment_lines.clear();
        }
    }

    // Second pass: parse YAML with serde_yaml
    let yaml: serde_yaml::Value = serde_yaml::from_str(content)?;

    match yaml {
        serde_yaml::Value::Mapping(map) => {
            let secrets = map
                .into_iter()
                .filter_map(|(k, v)| {
                    let key = k.as_str()?.to_string();
                    let name = key;

                    // let formatted_name = key
                    //     .to_uppercase()
                    //     .replace(|c: char| !c.is_alphanumeric(), "_");

                    let value = match v {
                        serde_yaml::Value::String(s) => Some(s),
                        serde_yaml::Value::Null => Some(String::new()),
                        serde_yaml::Value::Number(n) => Some(n.to_string()),
                        serde_yaml::Value::Bool(b) => Some(b.to_string()),
                        // Handle multiline strings (block style)
                        serde_yaml::Value::Mapping(m)
                            if m.contains_key(&serde_yaml::Value::String("|\n".to_string())) =>
                        {
                            m.get(&serde_yaml::Value::String("|\n".to_string()))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        }
                        _ => None,
                    }?;

                    Some(Secret {
                        name: name.clone(),
                        value,
                        comment: comments.remove(&name),
                    })
                })
                .collect::<Vec<Secret>>();

            Ok(secrets)
        }
        _ => bail!("YAML content must be a mapping of key-value pairs"),
    }
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

        let is_empty = trimmed.len() == 0;
        let is_comment = trimmed.starts_with("#");

        if !is_empty && !is_comment {
            match item.split_once(delimiter) {
                Some((name, value)) => {
                    let uppercase_name = name.to_uppercase();
                    let formatted_name = regex.replace_all(&uppercase_name, "_").trim().to_owned();
                    let formatted_value = value.trim().to_owned();

                    let comment = match index == 0 {
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
                        comment,
                        name: format!("{}", formatted_name),
                        value: format!("{}", formatted_value),
                    };

                    secrets.push(secret);
                }
                None => {
                    // NOTE: do nothing
                }
            }
        }
    }

    Ok(secrets)
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

pub fn extract_unique_references_from_secret(secret_value: &str) -> LinkedHashSet<String> {
    // Define the regular expression to match ${...}
    let regex = Regex::new(r"\$\{(.*?)\}").unwrap();
    // Create a HashSet to store unique references
    let mut refs = LinkedHashSet::new();

    // Iterate over all matches
    for cap in regex.captures_iter(secret_value) {
        // cap[1] contains the captured group inside ${}
        refs.insert_if_absent(cap[1].to_string());
    }

    return refs;
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

// Helper function to remove newlines from start and end of string
pub fn remove_str_outer_newlines(s: &str) -> String {
    // First unescape any escaped newlines
    let unescaped = s.replace("\\n", "\n");
    let processed = unescaped.trim_start_matches('\n').trim_end_matches('\n');
    // Re-escape newlines in the result
    processed.replace("\n", "\\n")
}

pub fn format_secret_comment(comment: &str, remove_outer_newlines: bool) -> String {
    // First unescape any escaped newlines
    let unescaped = comment.replace("\\n", "\n");
    let trimmed_lines: Vec<&str> = unescaped.lines().map(str::trim_end).collect();
    let joined = trimmed_lines.join("\n");

    match remove_outer_newlines {
        true => remove_str_outer_newlines(&joined),
        false => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cmd::config::SecretsOutputFormat,
        models::secrets::{Secret, SecretOptional},
    };

    #[test]
    fn yaml_format_keeps_commented_secret_entries_tightly_grouped() {
        let secrets = vec![
            Secret {
                name: "API_URL".to_string(),
                value: "http://localhost:5000".to_string(),
                comment: Some("comment".to_string()),
            },
            Secret {
                name: "DATABASE_URL".to_string(),
                value: "test".to_string(),
                comment: Some("comment".to_string()),
            },
            Secret {
                name: "PROD".to_string(),
                value: "false".to_string(),
                comment: None,
            },
        ];

        let formatted = format_secrets(secrets, &SecretsOutputFormat::Yaml);

        assert_eq!(
            formatted,
            "# comment\nAPI_URL: http://localhost:5000\n\n# comment\nDATABASE_URL: test\n\nPROD: false"
        );
    }

    #[test]
    fn list_format_uses_single_blank_line_between_commented_optional_entries() {
        let secrets = vec![
            SecretOptional {
                name: "ADMIN_API_KEY".to_string(),
                value: Some("".to_string()),
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "API_URL".to_string(),
                value: Some("http://localhost:5000".to_string()),
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "DATABASE_URL".to_string(),
                value: Some("test".to_string()),
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "PROD".to_string(),
                value: Some("false".to_string()),
                comment: None,
            },
        ];

        let formatted = format_optional_secrets(secrets, &SecretsOutputFormat::Plain, false);

        assert_eq!(
            formatted,
            "# comment\nADMIN_API_KEY: \n\n# comment\nAPI_URL: http://localhost:5000\n\n# comment\nDATABASE_URL: test\n\nPROD: false"
        );
    }

    #[test]
    fn yaml_format_keeps_commented_optional_entries_tightly_grouped() {
        let secrets = vec![
            SecretOptional {
                name: "ADMIN_API_KEY".to_string(),
                value: None,
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "API_URL".to_string(),
                value: Some("http://localhost:5000".to_string()),
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "DATABASE_URL".to_string(),
                value: Some("test".to_string()),
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "PROD".to_string(),
                value: Some("false".to_string()),
                comment: None,
            },
        ];

        let formatted = format_optional_secrets(secrets, &SecretsOutputFormat::Yaml, false);

        assert_eq!(
            formatted,
            "# comment\nADMIN_API_KEY: \n\n# comment\nAPI_URL: http://localhost:5000\n\n# comment\nDATABASE_URL: test\n\nPROD: false"
        );
    }

    #[test]
    fn list_format_masks_optional_entries_without_values_when_requested() {
        let secrets = vec![
            SecretOptional {
                name: "ADMIN_API_KEY".to_string(),
                value: None,
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "PROD".to_string(),
                value: Some("false".to_string()),
                comment: None,
            },
        ];

        let formatted = format_optional_secrets(secrets, &SecretsOutputFormat::Plain, true);

        assert_eq!(formatted, "# comment\nADMIN_API_KEY: [hidden]\n\nPROD: false");
    }

    #[test]
    fn yaml_format_masks_optional_entries_without_values_when_requested() {
        let secrets = vec![
            SecretOptional {
                name: "ADMIN_API_KEY".to_string(),
                value: None,
                comment: Some("comment".to_string()),
            },
            SecretOptional {
                name: "PROD".to_string(),
                value: Some("false".to_string()),
                comment: None,
            },
        ];

        let formatted = format_optional_secrets(secrets, &SecretsOutputFormat::Yaml, true);

        assert_eq!(formatted, "# comment\nADMIN_API_KEY: [hidden]\n\nPROD: false");
    }

    #[test]
    fn dotenv_parses_value_ending_with_escaped_quote_without_offsetting_next_secrets() {
        let content = "FIRST=\"abc\\\"\"\nSECOND=two\nTHIRD=three".to_string();

        let parsed = parse_dotenv_secrets_from_str(&content).unwrap();

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].name, "FIRST");
        assert_eq!(parsed[0].value, "abc\"");
        assert_eq!(parsed[1].name, "SECOND");
        assert_eq!(parsed[1].value, "two");
        assert_eq!(parsed[2].name, "THIRD");
        assert_eq!(parsed[2].value, "three");
    }

    #[test]
    fn ends_with_unescaped_quote_handles_even_and_odd_backslashes() {
        assert!(ends_with_unescaped_quote("\"abc\""));
        assert!(!ends_with_unescaped_quote("\"abc\\\""));
        assert!(ends_with_unescaped_quote("\"abc\\\\\""));
    }

    #[test]
    fn dotenv_trims_spaces_around_name_before_equal_sign() {
        let content = "INTERNAL_HEALTH_CHECK_TOKEN_HASH = \"xxx\"".to_string();

        let parsed = parse_dotenv_secrets_from_str(&content).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "INTERNAL_HEALTH_CHECK_TOKEN_HASH");
        assert_eq!(parsed[0].value, "xxx");
    }

    #[test]
    fn dotenv_multiline_closing_quote_on_own_line_does_not_swallow_next_secret() {
        let content = "PRIVATE_KEY=\"\n-----BEGIN RSA PRIVATE KEY-----\nXXX\n-----END RSA PRIVATE KEY-----\n\"\n\nAWS_REGION=\"eu-central-1\"".to_string();

        let parsed = parse_dotenv_secrets_from_str(&content).unwrap();

        assert_eq!(parsed.len(), 2);

        assert_eq!(parsed[0].name, "PRIVATE_KEY");
        assert_eq!(
            parsed[0].value,
            "-----BEGIN RSA PRIVATE KEY-----\nXXX\n-----END RSA PRIVATE KEY-----"
        );
        assert_eq!(parsed[1].name, "AWS_REGION");
        assert_eq!(parsed[1].value, "eu-central-1");
    }

    #[test]
    fn dotenv_multiline_quote_only_wrapper_trims_outer_empty_lines() {
        let content = "PRIVATE_KEY=\"\n\nLINE_1\nLINE_2\n\n\"\n".to_string();

        let parsed = parse_dotenv_secrets_from_str(&content).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "PRIVATE_KEY");
        assert_eq!(parsed[0].value, "LINE_1\nLINE_2");
    }
}
