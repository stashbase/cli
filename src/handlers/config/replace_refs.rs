// use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{config::config, models::config::UpdateConfig};

pub fn set_replace_refs_config(enabled: Option<bool>) {
    if let None = enabled {
        eprintln!(
            "{} {}",
            "Error:".red(),
            "No 'enabled' boolean value provided"
        );
        return;
    }

    let enabled = enabled.unwrap();

    let res = config::update_config(UpdateConfig {
        api_key: None,
        output_format: None,
        replace_refs: None,
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!(
            "{} {}",
            "✔".green(),
            "Default replace-refs config has been set"
        );
        eprintln!("{}", msg);
    }
}

pub fn print_replace_refs_config(enabled: &Option<bool>) {
    if let Some(enabled) = enabled {
        if *enabled {
            println!("Replace refs: true");
        } else {
            println!("Replace refs: false");
        }
    }
}
