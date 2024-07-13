use owo_colors::OwoColorize;

use crate::{
    cmd::config::SecretsOutputFormat,
    config::config,
    models::config::{OutputFormatConfig, UpdateConfig},
};

pub fn set_default_secrets_output_format(output_format: SecretsOutputFormat) {
    let res = config::update_config(UpdateConfig {
        api_key: None,
        expand_refs: None,
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
