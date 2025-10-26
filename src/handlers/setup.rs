use anyhow::Result;

use crate::{
    cmd::config::{OutputFormat, SecretsOutputFormat},
    config::config,
    models::config::{Config, OutputFormatConfig, UpdateConfig},
    utils::interaction::{confirm_opt, input_password},
};
use dialoguer::{theme::ColorfulTheme, Select};

pub fn setup(existing_config: Config) -> Result<()> {
    // Implementation for setup
    //
    eprintln!("Welcome! This will guide you through configuring the Stashbase CLI.");

    let has_api_key = existing_config.api_key.is_some();

    let api_key_prompt = if has_api_key {
        "Enter your API key (leave empty to keep existing)"
    } else {
        "Enter your API key"
    };

    let api_key = input_password(api_key_prompt);

    let current_output_format = existing_config.ouput_format;

    let (output_format, secrets_output_format) = match current_output_format {
        Some(format) => (format.general, format.secrets),
        None => (None, None),
    };

    let new_output_format = select_output_format(output_format);
    let new_secrets_output_format = select_secrets_output_format(secrets_output_format);

    let expand_refs = select_expand_secret_references();

    let updated_config = UpdateConfig {
        api_key,
        expand_refs,
        output_format: Some(OutputFormatConfig {
            general: Some(new_output_format),
            secrets: Some(new_secrets_output_format),
        }),
    };

    config::update_config(updated_config)?;
    eprintln!("\nSetup completed.");

    Ok(())
}

fn select_output_format(current: Option<OutputFormat>) -> OutputFormat {
    let theme = ColorfulTheme::default();

    let items = ["List (default)", "Table", "JSON"];
    let default_index = match current.unwrap_or_default() {
        OutputFormat::List => 0,
        OutputFormat::Table => 1,
        OutputFormat::Json => 2,
    };

    let selection = Select::with_theme(&theme)
        .with_prompt("Select default general output format")
        .items(&items)
        .default(default_index)
        .interact()
        .unwrap_or(default_index);

    match selection {
        0 => OutputFormat::List,
        1 => OutputFormat::Table,
        2 => OutputFormat::Json,
        _ => OutputFormat::List,
    }
}

fn select_secrets_output_format(current: Option<SecretsOutputFormat>) -> SecretsOutputFormat {
    let theme = ColorfulTheme::default();

    let items = ["List (default)", "Table", "Dotenv", "YAML", "JSON"];
    let default_index = match current.unwrap_or_default() {
        SecretsOutputFormat::List => 0,
        SecretsOutputFormat::Table => 1,
        SecretsOutputFormat::Dotenv => 2,
        SecretsOutputFormat::Yaml => 3,
        SecretsOutputFormat::Json => 4,
    };

    let selection = Select::with_theme(&theme)
        .with_prompt("Select default output format for secrets")
        .items(&items)
        .default(default_index)
        .interact()
        .unwrap_or(default_index);

    match selection {
        0 => SecretsOutputFormat::List,
        1 => SecretsOutputFormat::Table,
        2 => SecretsOutputFormat::Dotenv,
        3 => SecretsOutputFormat::Yaml,
        4 => SecretsOutputFormat::Json,
        _ => SecretsOutputFormat::List,
    }
}

pub fn select_expand_secret_references() -> Option<bool> {
    confirm_opt("Expand secret references by default")
}
