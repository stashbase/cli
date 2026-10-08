use anyhow::Result;

use crate::{
    config::config, exit::ReportedFailure, models::config::UpdateConfig,
    telemetry::event::ErrorKind, utils::output::ColorizeIfColoredOutput,
};

pub fn set_expand_refs_config(enabled: Option<bool>) -> Result<()> {
    if let None = enabled {
        eprintln!(
            "{} {}",
            "Error:".red_if_tty_stderr(),
            "No 'enabled' boolean value provided."
        );
        return Err(ReportedFailure::new(ErrorKind::Validation));
    }

    let enabled = enabled.unwrap();

    let res = config::update_config(UpdateConfig {
        api_key: None,
        output_format: None,
        expand_refs: Some(enabled),
    });

    if let Err(err) = res {
        eprintln!("{} {}", "Error:".red_if_tty_stderr(), err);
        return Err(ReportedFailure::new(ErrorKind::Other));
    }

    let msg = format!("Default expand-refs config set.");
    println!("{}", msg);
    Ok(())
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
