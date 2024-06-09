use owo_colors::OwoColorize;

use crate::{
    cmd::configs::SecretsOutputFormat,
    config::config,
    models::config::{OutputFormatConfig, UpdateConfig},
};

pub fn set_default_output_format_secrets(output_format: SecretsOutputFormat) {
    let res = config::update_config(UpdateConfig {
        api_key: None,
        output_format: Some(OutputFormatConfig {
            secrets: Some(output_format),
            general: None,
        }),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red(), err);
    } else {
        let msg = format!(
            "{} {}",
            "✔".green(),
            "Default secrets output format has been set"
        );
        eprintln!("{}", msg);
    }
}

pub fn print_default_secrets_output_format(output_format: &SecretsOutputFormat) {
    println!("Default output format (secrets): {}", output_format);
}
