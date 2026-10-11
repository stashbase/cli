use anyhow::Result;

use crate::{
    cmd::config::OutputFormat,
    config::config,
    exit::ReportedFailure,
    models::config::{OutputFormatConfig, UpdateConfig},
    telemetry::event::ErrorKind,
    utils::output::ColorizeIfColoredOutput,
};

pub fn set_default_output_format(output_format: OutputFormat) -> Result<()> {
    let res = config::update_config(UpdateConfig {
        api_key: None,
        expand_refs: None,
        output_format: Some(OutputFormatConfig {
            secrets: None,
            general: Some(output_format),
        }),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red_if_tty_stderr(), err);
        return Err(ReportedFailure::new(ErrorKind::Other));
    }

    let msg = format!("{} {}", "✔".green_if_tty(), "Default output format set.");
    println!("{}", msg);
    Ok(())
}

pub fn print_default_output_format(output_format: &OutputFormat) {
    println!("Default output format: {}.", output_format);
}
