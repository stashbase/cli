use owo_colors::OwoColorize;

use crate::{config::config, models::config::UpdateConfig};

pub fn set_expand_refs_config(enabled: Option<bool>) {
    if let None = enabled {
        eprintln!(
            "{} {}",
            "Error:".red(),
            "No 'enabled' boolean value provided."
        );
        return;
    }

    let enabled = enabled.unwrap();

    let res = config::update_config(UpdateConfig {
        api_key: None,
        output_format: None,
        expand_refs: Some(enabled),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("Default expand-refs config set.");
        eprintln!("{}", msg);
    }
}

pub fn print_expand_refs_config(enabled: &Option<bool>) {
    if let Some(enabled) = enabled {
        if *enabled {
            println!("Expand refs: true.");
        } else {
            println!("Expand refs: false.");
        }
    }
}
