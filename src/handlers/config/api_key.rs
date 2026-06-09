// use anyhow::Result;

use std::io::{self, Read};

use crate::{
    config::{config, secure_store},
    utils::{interaction::input_password, output::ColorizeIfColoredOutput},
};

pub fn set_api_key(read_from_stdin: bool) {
    let api_key_value = if read_from_stdin {
        match read_api_key_from_stdin() {
            Ok(value) => value,
            Err(message) => {
                eprintln!("{}", message.red_if_tty_stderr());
                return;
            }
        }
    } else {
        match input_password("Enter your API key") {
            Some(value) => value,
            None => {
                eprintln!("{}", "No API key entered. Aborted.".red_if_tty_stderr());
                return;
            }
        }
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
            "Secure key storage unavailable, using file-permissions-based config fallback."
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

fn read_api_key_from_stdin() -> Result<String, &'static str> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|_| "Failed to read API key from stdin.")?;

    let value = buffer.trim().to_string();
    if value.is_empty() {
        Err("No API key provided on stdin. Aborted.")
    } else {
        Ok(value)
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
