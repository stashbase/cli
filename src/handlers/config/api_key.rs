// use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{config::config, models::config::UpdateConfig};

pub fn set_api_key(api_key: String) {
    let res = config::update_config(UpdateConfig {
        api_key: Some(api_key),
        output_format: None,
        replace_refs: None,
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "API Key has been set");
        eprintln!("{}", msg);
    }
}

pub fn print_api_key(api_key: &Option<String>, full: bool) {
    if let Some(api_key) = api_key {
        if full {
            println!("{}", api_key);
        } else {
            let formatted = get_first_3_and_last_5(api_key);

            if let Some(formatted) = formatted {
                println!("{}...{}", formatted.0, formatted.1);
            } else {
                println!("{}", api_key);
            }
        }
    } else {
        eprintln!("{}", "No API key set");
    }
}

fn get_first_3_and_last_5(s: &str) -> Option<(String, String)> {
    if s.len() < 8 {
        return None; // The string is too short to extract both parts.
    }

    let first_3 = &s[..3];
    let last_5 = &s[s.len() - 5..];

    Some((first_3.to_string(), last_5.to_string()))
}
