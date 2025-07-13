use std::path::Path;

use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{FormatSecrets, ValidateSecrets},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{interaction, secrets::read_secrets_from_file, spinner::request_spinner},
};

pub struct HandleUploadSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub file_path: String,
    pub format: Option<SecretsFileFormat>,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_upload_secrets(args: HandleUploadSecretsArgs) -> Result<()> {
    let HandleUploadSecretsArgs {
        api_key,
        project,
        environment,
        file_path,
        format,
        json_format,
        silent,
    } = args;

    let path = Path::new(&file_path);
    debug!("Path: {:#?}", path);

    let file_exists = path.exists();
    debug!("File exists: {}", file_exists);

    if !file_exists {
        let err_msg = format!("{} {}", "Error reading file:".red(), "file does not exist.");
        bail!(err_msg);
    }

    // let secrets_res = read_secrets_file(path);
    //
    let target_format = match format {
        Some(format) => format,
        None => {
            if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
                SecretsFileFormat::Yaml
            } else if file_path.ends_with(".json") {
                SecretsFileFormat::Json
            } else {
                SecretsFileFormat::Dotenv
            }
        }
    };

    let secrets_res = read_secrets_from_file(path, &target_format);

    if let Err(err) = secrets_res {
        let err =
            InputValidationError::Secrets(SecretsInputValidationError::ReadFile(err.to_string()));

        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    let mut secrets = secrets_res.unwrap();

    // format secrets for input
    secrets.format();

    if secrets.is_empty() {
        if json_format {
            let error_json = serde_json::json!({
                "error": {
                    "message": "Nothing to upload: no secrets found."
                }
            });
            let json = to_colored_json_auto(&error_json).unwrap();

            eprintln!();
            println!("{}", json);
        }

        return Ok(());
    }

    // validate secrets
    if let Err(err) = secrets.validate() {
        if !silent {
            eprintln!();
        }
        let error_output = err.format_error_output(json_format)?;

        bail!(error_output);
    }

    let reference_warnings = secrets.get_reference_warnings();

    if !reference_warnings.is_empty() && !silent {
        eprint!("{}", reference_warnings);
    }

    if !silent {
        let info = format!("Number of secrets to upload: {}", secrets.len());
        eprintln!("{}", info);

        let confirm = interaction::confirm_opt("Are you sure you want to continue?");

        if confirm.is_none() || (confirm.unwrap() == false) {
            return Ok(());
        }

        eprintln!();
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };
    let res = secrets::set_sercrets(api_key, project, environment, &secrets).await;
    debug!("{:#?}", res);

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }
                println!("{{}}");
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Secrets uploaded.");
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
