use std::path::Path;

use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    handlers::secrets::pretty_print::print_secret_name_list,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{FormatSecrets, UpsertSecretsResponse, ValidateSecrets},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction,
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
        secrets::read_secrets_from_file,
        spinner::request_spinner,
    },
};

pub struct HandleUploadSecretsArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub file_path: String,
    pub format: Option<SecretsFileFormat>,
    pub ignore_comments: bool,
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
        ignore_comments,
        json_format,
        silent,
    } = args;

    let path = Path::new(&file_path);
    debug!("Path: {:#?}", path);

    let file_exists = path.exists();
    debug!("File exists: {}", file_exists);

    if !file_exists {
        let err_msg = format!(
            "{} {}",
            "Error reading file:".red_if_tty_stderr(),
            "file does not exist."
        );
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

    if ignore_comments {
        secrets = secrets
            .into_iter()
            .map(|secret| secret.without_comment())
            .collect();
    }

    // format secrets for input
    secrets.format();

    if secrets.is_empty() {
        if json_format {
            let error_json = serde_json::json!({
                "error": {
                    "message": "Nothing to upload: no secrets found."
                }
            });
            let json = get_formatted_json_string(&error_json, false).unwrap();

            eprintln!();
            eprintln!("{}", json);
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
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<UpsertSecretsResponse>(&text);

                match json_data {
                    Ok(data) => {
                        if json_format {
                            let json_str = get_formatted_json_string(&data, true).unwrap();

                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }

                            println!("{}", json_str);
                            return Ok(());
                        }

                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let created_count = data.created_secrets.len();
                        let updated_count = data.updated_secrets.len();

                        if silent {
                            println!("Created: {}", created_count);
                            println!("Updated: {}", updated_count);
                        } else {
                            match (created_count, updated_count) {
                                (0, 0) => println!("No secrets changed."),
                                _ => {
                                    println!("Created: {}", created_count);
                                    println!("Updated: {}", updated_count);
                                    print_secret_name_list(
                                        "Created secrets:",
                                        &data.created_secrets,
                                    );
                                    print_secret_name_list(
                                        "Updated secrets:",
                                        &data.updated_secrets,
                                    );
                                }
                            }
                        }
                    }
                    Err(_) => {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let error_str =
                            crate::models::api_client::OutputError::failed_to_deserialize_response_body()
                                .format_error_output(json_format)?;
                        bail!(error_str);
                    }
                }
            }
            None => {
                if json_format {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                    println!("{{}}");
                } else if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Secrets uploaded.");
                }
            }
        },
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
