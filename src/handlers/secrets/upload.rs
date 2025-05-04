use std::path::Path;

use anyhow::{bail, Result};
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
}

pub async fn handle_upload_secrets(args: HandleUploadSecretsArgs) -> Result<()> {
    let HandleUploadSecretsArgs {
        api_key,
        project,
        environment,
        file_path,
        format,
        json_format,
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

        eprintln!();
        bail!(err);
    }

    let mut secrets = secrets_res.unwrap();

    // format secrets for input
    secrets.format();

    if secrets.is_empty() {
        let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found.");
        eprintln!("{}", msg);

        return Ok(());
    }

    // validate secrets
    if let Err(err) = secrets.validate() {
        eprintln!();
        bail!(err);
    }

    let reference_warnings = secrets.get_reference_warnings();

    if !reference_warnings.is_empty() {
        eprint!("{}", reference_warnings);
    }

    let info = format!("Number of secrets to upload: {}", secrets.len());
    eprintln!("{}", info);

    let confirm = interaction::confirm_opt("Are you sure you want to continue?");

    if confirm.is_none() || (confirm.unwrap() == false) {
        return Ok(());
    }

    eprintln!();

    let mut spinner = request_spinner();
    let res = secrets::set_sercrets(api_key, project, environment, &secrets).await;
    debug!("{:#?}", res);

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                spinner.stop_and_persist("", "");
                println!("{{}}");
            } else {
                spinner.stop_with_message("Secrets uploaded.");
            }
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
