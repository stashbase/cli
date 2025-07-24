use std::path::Path;

use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        secrets::SecretOptional,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{secrets::read_secrets_from_file, spinner::request_spinner},
};

pub struct HandleSecretsDiffArgs {
    pub silent: bool,
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub file_path: String,
    pub format: Option<SecretsFileFormat>,
    pub json_format: bool,
    pub expand_refs: bool,
}

pub async fn handle_secrets_diff(args: HandleSecretsDiffArgs) -> Result<()> {
    let HandleSecretsDiffArgs {
        silent,
        api_key,
        project,
        environment,
        file_path,
        format,
        json_format,
        expand_refs,
    } = args;

    let path = Path::new(&file_path);

    let file_exists = path.exists();

    if !file_exists {
        // TODO: validation error
        bail!("File does not exist: {}", file_path);
    }

    let target_format = {
        if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
            SecretsFileFormat::Yaml
        } else if file_path.ends_with(".json") {
            SecretsFileFormat::Json
        } else {
            SecretsFileFormat::Dotenv
        }
    };

    let secrets_res = read_secrets_from_file(path, &target_format);

    if let Err(err) = secrets_res {
        let err =
            InputValidationError::Secrets(SecretsInputValidationError::ReadFile(err.to_string()));

        let error_output = err.format_error_output(false)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    let secrets = secrets_res.unwrap();

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let remote_secrets_res =
        secrets::list(api_key, project, environment, false, None, expand_refs).await;

    if let Err(err) = remote_secrets_res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(false)?;
        bail!(error_output);
    }

    let remote_secrets = remote_secrets_res.unwrap();

    match remote_secrets {
        GetRequestApiResponse::Ok(data) => {
            let names = serde_json::from_str::<Vec<SecretOptional>>(&data.text);

            match names {
                Ok(remote_secrets) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                }
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
