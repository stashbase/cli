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
                    text_to_print.push_str(&format!("{}", p))
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
