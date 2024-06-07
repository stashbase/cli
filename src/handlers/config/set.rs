// use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{cmd::configs::OutputFormat, config::config, models::config::UpdateConfig};

pub fn set_api_key(api_key: String) {
    let res = config::update_config(UpdateConfig {
        api_key: Some(api_key),
        output_format: None,
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "API Key has been set");
        eprintln!("{}", msg);
    }
}

pub fn set_default_output_format(output_format: OutputFormat) {
    let res = config::update_config(UpdateConfig {
        api_key: None,
        output_format: Some(output_format),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "Default output format has been set");
        eprintln!("{}", msg);
    }
}
