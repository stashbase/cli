// use anyhow::Result;

use crate::{
    config::{config, secure_store},
    utils::{interaction::input_password, output::ColorizeIfColoredOutput},
};

pub fn set_api_key(api_key: Option<String>) {
    // If API key argument not provided, prompt the user
    let api_key_value = match api_key {
        Some(key) => key,
        None => match input_password("Enter your API key") {
            Some(value) => value,
            None => {
                eprintln!("{}", "No API key entered. Aborted.".red_if_tty_stderr());
                return;
            }
        },
    };

    let store_res = secure_store::set_api_key(&api_key_value);

    if let Err(store_err) = store_res {
        let fallback_res = config::update_config(crate::models::config::UpdateConfig {
            api_key: Some(api_key_value),
            output_format: None,
            expand_refs: None,
        });

        if let Err(fallback_err) = fallback_res {
            eprintln!("{} {}", "Error:".red_if_tty_stderr(), fallback_err);
            return;
        }

        eprintln!(
            "{} {}",
            "Warning:".yellow_if_tty_stderr(),
            "Secure key storage unavailable, using encrypted-by-permissions config fallback."
        );
        eprintln!(
            "{} {}",
            "Reason:".yellow_if_tty_stderr(),
            store_err.to_string()
        );
    } else {
        let _ = config::clear_legacy_api_key();
        println!("API key set.");
    }
}

pub fn print_api_key(api_key: &Option<String>) {
    if let Some(api_key) = api_key {
        let formatted = get_first_3_and_last_5(api_key);

        if let Some(formatted) = formatted {
            println!("{}...{}", formatted.0, formatted.1);
        } else {
            println!("***");
        }
    } else {
        eprintln!("{}", "No API key set.");
    }
}

pub fn get_first_3_and_last_5(s: &str) -> Option<(String, String)> {
    if s.len() < 8 {
        return None; // The string is too short to extract both parts.
    }

    let first_3 = &s[..3];
    let last_5 = &s[s.len() - 5..];

    Some((first_3.to_string(), last_5.to_string()))
}
