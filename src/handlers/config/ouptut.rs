// use anyhow::Result;
use owo_colors::OwoColorize;

use crate::{
    cmd::configs::OutputFormat,
    config::config,
    models::config::{OutputFormatConfig, UpdateConfig},
};

pub fn set_default_output_format(output_format: OutputFormat) {
    let res = config::update_config(UpdateConfig {
        api_key: None,
        output_format: Some(OutputFormatConfig {
            secrets: None,
            general: Some(output_format),
        }),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!("{} {}", "✔".green(), "Default output format has been set");
        eprintln!("{}", msg);
    }
}

pub fn print_default_output_format(output_format: &OutputFormat) {
    println!("Default output format: {}", output_format);
}
