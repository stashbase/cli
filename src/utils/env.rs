use std::{collections::HashMap, env};

use crate::models::secrets::SecretWithoutComment;

/// Returns the value of an environment variable if set and non-empty.
pub fn get_env_var(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(val) if !val.trim().is_empty() => Some(val.trim().to_string()),
        _ => None,
    }
}

/// Convenience helper to get the Stashbase API key from environment.
pub fn get_stashbase_api_key() -> Option<String> {
    get_env_var("STASHBASE_API_KEY")
}

pub fn expand_and_inject_env(parsed: &mut [SecretWithoutComment]) -> HashMap<String, String> {
    let process_env = env::vars().collect::<HashMap<_, _>>();
    expand_and_inject_env_with_process_env(parsed, &process_env)
}

fn expand_and_inject_env_with_process_env(
    parsed: &mut [SecretWithoutComment],
    process_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut running_parsed = HashMap::<String, String>::new();

    for secret in parsed.iter_mut() {
        let current_value = secret.value.clone();
        let process_value = process_env.get(&secret.name);

        let value = match process_value {
            Some(process_value) if process_value != &current_value => process_value.clone(),
            _ => expand_value(&current_value, process_env, &running_parsed),
        };

        let normalized = resolve_escape_sequences(&value);
        secret.value = normalized.clone();
        running_parsed.insert(secret.name.clone(), normalized);
    }

    running_parsed
}

fn resolve_escape_sequences(value: &str) -> String {
    value.replace("\\$", "$")
}

fn expand_value(
    value: &str,
    process_env: &HashMap<String, String>,
    running_parsed: &HashMap<String, String>,
) -> String {
    let mut result = value.to_string();
    let mut seen = std::collections::HashSet::<String>::new();

    while let Some((start, end, expression)) = find_next_expansion(&result) {
        seen.insert(result.clone());

        let (key, splitter, default_value) = split_expression(&expression);
        let resolved_value = process_env.get(key).or_else(|| running_parsed.get(key));
        let is_set = resolved_value.is_some();
        let is_non_empty = resolved_value.is_some_and(|value| !value.is_empty());
        let use_alt_default = splitter == Some(":+");
        let use_alt_empty_default = splitter == Some("+");
        let use_default_when_empty = splitter == Some(":-");

        let replacement = if use_alt_default {
            if is_non_empty {
                default_value.to_string()
            } else {
                String::new()
            }
        } else if use_alt_empty_default {
            if is_set {
                default_value.to_string()
            } else {
                String::new()
            }
        } else if is_set && (!use_default_when_empty || is_non_empty) {
            let value = resolved_value.unwrap();
            if seen.contains(value) {
                default_value.to_string()
            } else {
                value.clone()
            }
        } else {
            default_value.to_string()
        };

        result.replace_range(start..end, &replacement);

        if running_parsed.get(key).is_some_and(|value| result == *value) {
            break;
        }
    }

    result
}

fn split_expression(expression: &str) -> (&str, Option<&'static str>, &str) {
    let bytes = expression.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let remainder = &expression[index..];

        if remainder.starts_with(":+") {
            return (&expression[..index], Some(":+"), &expression[index + 2..]);
        }
        if remainder.starts_with(":-") {
            return (&expression[..index], Some(":-"), &expression[index + 2..]);
        }
        if remainder.starts_with('+') {
            return (&expression[..index], Some("+"), &expression[index + 1..]);
        }
        if remainder.starts_with('-') {
            return (&expression[..index], Some("-"), &expression[index + 1..]);
        }

        index += 1;
    }

    (expression, None, "")
}

fn find_next_expansion(input: &str) -> Option<(usize, usize, String)> {
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'$' && (index == 0 || bytes[index - 1] != b'\\') {
            if index + 1 < bytes.len() && bytes[index + 1] == b'{' {
                let expression_start = index + 2;
                let mut cursor = expression_start;

                while cursor < bytes.len() {
                    if bytes[cursor] == b'}' {
                        let expression = &input[expression_start..cursor];
                        if !expression.is_empty()
                            && !expression.contains('{')
                            && !expression.contains('}')
                        {
                            return Some((index, cursor + 1, expression.to_string()));
                        }
                        break;
                    }
                    cursor += 1;
                }
            } else if index + 1 < bytes.len() && is_var_start(bytes[index + 1] as char) {
                let mut cursor = index + 2;
                while cursor < bytes.len() && is_var_char(bytes[cursor] as char) {
                    cursor += 1;
                }

                return Some((index, cursor, input[index + 1..cursor].to_string()));
            }
        }

        index += 1;
    }

    None
}

fn is_var_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_var_char(ch: char) -> bool {
    is_var_start(ch) || ch.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::expand_and_inject_env_with_process_env;
    use crate::models::secrets::SecretWithoutComment;

    #[test]
    fn expands_empty_values_with_shell_compatible_operator_semantics() {
        let process_env = HashMap::from([(String::from("EMPTY"), String::from(""))]);
        let mut parsed = vec![
            SecretWithoutComment {
                name: "DIRECT".to_string(),
                value: "$EMPTY".to_string(),
            },
            SecretWithoutComment {
                name: "DEFAULT_IF_UNSET_ONLY".to_string(),
                value: "${EMPTY-fallback}".to_string(),
            },
            SecretWithoutComment {
                name: "DEFAULT_IF_EMPTY".to_string(),
                value: "${EMPTY:-fallback}".to_string(),
            },
            SecretWithoutComment {
                name: "ALT_IF_SET".to_string(),
                value: "${EMPTY+present}".to_string(),
            },
            SecretWithoutComment {
                name: "ALT_IF_NON_EMPTY".to_string(),
                value: "${EMPTY:+present}".to_string(),
            },
        ];

        let injected = expand_and_inject_env_with_process_env(&mut parsed, &process_env);

        assert_eq!(parsed[0].value, "");
        assert_eq!(parsed[1].value, "");
        assert_eq!(parsed[2].value, "fallback");
        assert_eq!(parsed[3].value, "present");
        assert_eq!(parsed[4].value, "");

        assert_eq!(injected.get("DIRECT"), Some(&"".to_string()));
        assert_eq!(injected.get("DEFAULT_IF_UNSET_ONLY"), Some(&"".to_string()));
        assert_eq!(injected.get("DEFAULT_IF_EMPTY"), Some(&"fallback".to_string()));
        assert_eq!(injected.get("ALT_IF_SET"), Some(&"present".to_string()));
        assert_eq!(injected.get("ALT_IF_NON_EMPTY"), Some(&"".to_string()));
    }

    #[test]
    fn injects_empty_stored_secrets_into_runtime_env() {
        let process_env = HashMap::new();
        let mut parsed = vec![SecretWithoutComment {
            name: "EMPTY_SECRET".to_string(),
            value: "".to_string(),
        }];

        let injected = expand_and_inject_env_with_process_env(&mut parsed, &process_env);

        assert_eq!(parsed[0].value, "");
        assert!(injected.contains_key("EMPTY_SECRET"));
        assert_eq!(injected.get("EMPTY_SECRET"), Some(&"".to_string()));
    }

    #[test]
    fn prefers_explicitly_empty_runtime_env_values_over_parsed_values() {
        let process_env = HashMap::from([(String::from("API_KEY"), String::from(""))]);
        let mut parsed = vec![SecretWithoutComment {
            name: "API_KEY".to_string(),
            value: "from-parsed".to_string(),
        }];

        let injected = expand_and_inject_env_with_process_env(&mut parsed, &process_env);

        assert_eq!(parsed[0].value, "");
        assert_eq!(injected.get("API_KEY"), Some(&"".to_string()));
    }
}
