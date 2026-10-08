use anyhow::Result;

use crate::{
    cmd::config::SecretsOutputFormat,
    config::config,
    exit::ReportedFailure,
    models::config::{OutputFormatConfig, UpdateConfig},
    telemetry::event::ErrorKind,
    utils::output::ColorizeIfColoredOutput,
};

pub fn set_default_secrets_output_format(output_format: SecretsOutputFormat) -> Result<()> {
    let res = config::update_config(UpdateConfig {
        api_key: None,
        expand_refs: None,
        output_format: Some(OutputFormatConfig {
            secrets: Some(output_format),
            general: None,
        }),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red_if_tty_stderr(), err);
        return Err(ReportedFailure::new(ErrorKind::Other));
    }

    let msg = format!("Default secrets output format set.");
    println!("{}", msg);
    Ok(())
}

pub fn print_default_secrets_output_format(output_format: &SecretsOutputFormat) {
    println!("Default output format (secrets): {}.", output_format);
}
