use anyhow::bail;
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::{DeleteRequestApiResponse, OutputError, RequestApiOptionResponse},
        secrets::{DeleteAllSecretsResponse, DeleteSecretsResponse},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction,
        output::get_colored_json,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_names},
    },
};

pub struct HandleDeleteSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub names: Vec<String>,
    pub delete_all: bool,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

// ✓
pub async fn handle_delete_secrets(args: HandleDeleteSecretsArgs) -> anyhow::Result<()> {
    let HandleDeleteSecretsArgs {
        api_key,
        project,
        environment,
        delete_all,
        names,
        json_format,
        silent,
        force,
    } = args;

    if names.is_empty() && !delete_all {
        let secrets_error = SecretsInputValidationError::NoSecretsToDelete;
        let input_error = InputValidationError::Secrets(secrets_error);

        let error_output = input_error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    let validation_res = validate_input(&project, &environment, &names);

    if let Err(e) = validation_res {
        let error_output = e.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    if delete_all && !force {
        eprintln!(
            "{}",
            "All secrets in selected environment will be deleted.".red()
        );
    }

    let i = interaction::confirm_opt("Are you sure you want to continue?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }
    debug!("deleting secrets...:");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    match delete_all {
        true => {
            let res = secrets::delete_all(api_key, project, environment).await;

            if let Err(err) = res {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                error!("{:#?}", &err);

                let error_output = err.format_error_output(json_format)?;
                bail!(error_output);
            }

            let res = res.unwrap();

            match res {
                RequestApiOptionResponse::Ok(res) => {
                    match res.text {
                        Some(text) => {
                            //
                            let json_data = serde_json::from_str::<DeleteAllSecretsResponse>(&text);

                            match json_data {
                                Ok(d) => {
                                    if json_format {
                                        let json_str = get_colored_json(&d).unwrap();

                                        if let Some(mut spinner) = spinner {
                                            spinner.stop_and_persist("", "");
                                        }

                                        if !silent {
                                            println!("{}", json_str);
                                        }
                                    } else {
                                        if !silent {
                                            match d.deleted_count {
                                                0 => {
                                                    if let Some(mut spinner) = spinner {
                                                        spinner.stop_with_message(
                                                            "No secrets to delete.",
                                                        );
                                                    } else {
                                                        eprintln!("No secrets to delete.");
                                                    }
                                                }
                                                _ => {
                                                    let msg = format!(
                                                        "All secrets ({}) deleted.",
                                                        d.deleted_count
                                                    );

                                                    if let Some(mut spinner) = spinner {
                                                        spinner.stop_with_message(&format!(
                                                            "{} {}",
                                                            "✓".green(),
                                                            msg
                                                        ));
                                                    } else {
                                                        eprintln!("{}", msg);
                                                    }
                                                }
                                            }
                                        } else {
                                            // Silent mode: just stop spinner if any
                                            if let Some(mut spinner) = spinner {
                                                spinner.stop_and_persist("", "");
                                            }
                                        }
                                    }
                                }
                                Err(_) => {
                                    if let Some(mut spinner) = spinner {
                                        spinner.stop_and_persist("", "");
                                    }

                                    let error = OutputError::failed_to_deserialize_response_body();
                                    let formatted_err = error.format_error_output(json_format)?;

                                    bail!(formatted_err);
                                }
                            }
                        }
                        None => {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }

                            let error = OutputError::failed_to_deserialize_response_body();
                            let formatted_err = error.format_error_output(json_format)?;

                            bail!(formatted_err);
                        }
                    }
                }
                RequestApiOptionResponse::Err(e) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let formatted_err = e.format_error_output(json_format)?;

                    bail!(formatted_err);
                }
            }
        }
        false => {
            let res = secrets::delete(api_key, project, environment, &names).await;

            if let Err(err) = res {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let formatted_err = err.format_error_output(json_format)?;
                bail!(formatted_err);
            }

            let res = res.unwrap();

            match res {
                RequestApiOptionResponse::Ok(res) => {
                    // all deleted
                    match res.text {
                        Some(text) => {
                            let json_data = serde_json::from_str::<DeleteSecretsResponse>(&text);
                            debug!("{:#?}", json_data);

                            match json_data {
                                Ok(data) => {
                                    if json_format {
                                        let json_str = get_colored_json(&data).unwrap();

                                        if let Some(mut spinner) = spinner {
                                            spinner.stop_and_persist("", "");
                                        }

                                        println!("{}", json_str);

                                        return Ok(());
                                    }

                                    let not_found_secrets = data.not_found_secrets;
                                    let not_found_len = not_found_secrets.len();

                                    debug!("{:#?}", not_found_secrets);

                                    if not_found_len > 0 {
                                        if let Some(mut spinner) = spinner {
                                            spinner.stop_and_persist("", "");
                                        }

                                        if !silent {
                                            let info_msg = format!(
                                                "{} {}",
                                                format!(
                                                    "{} {} {}",
                                                    "Secrets".red(),
                                                    format!("({})", not_found_len).red(),
                                                    "not found:".red()
                                                ),
                                                not_found_secrets.join(", ")
                                            );

                                            eprintln!("{}", info_msg);
                                        }

                                        let deleted_count = data.deleted_count;

                                        if deleted_count > 0 && !silent {
                                            let secrets_deleted: Vec<_> = names
                                                .into_iter()
                                                .filter(|k| {
                                                    not_found_secrets
                                                        .iter()
                                                        .find(|s| *s == k)
                                                        .is_none()
                                                })
                                                .collect();

                                            let msg = format!(
                                                "{} {}",
                                                format!(
                                                    "{} {} {}",
                                                    "Secrets".green(),
                                                    format!("({})", deleted_count).green(),
                                                    "deleted:".green()
                                                ),
                                                secrets_deleted.join(", ")
                                            );

                                            println!("{}", msg);
                                        }
                                    } else {
                                        if !silent {
                                            if let Some(mut spinner) = spinner {
                                                spinner
                                                    .stop_with_message("Selected secrets deleted.");
                                            } else {
                                                eprintln!("Selected secrets deleted.");
                                            }
                                        } else {
                                            if let Some(mut spinner) = spinner {
                                                spinner.stop_and_persist("", "");
                                            }
                                        }
                                    }
                                }
                                Err(_) => {
                                    if let Some(mut spinner) = spinner {
                                        spinner.stop_and_persist("", "");
                                    }

                                    let error = OutputError::failed_to_deserialize_response_body();
                                    let formatted_err = error.format_error_output(json_format)?;

                                    bail!(formatted_err);
                                }
                            }
                        }
                        None => {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }

                            let error = OutputError::failed_to_deserialize_response_body();
                            let formatted_err = error.format_error_output(json_format)?;

                            bail!(formatted_err);
                        }
                    }
                }
                RequestApiOptionResponse::Err(e) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let error_output = e.format_error_output(json_format)?;
                    bail!(error_output);
                }
            }
        }
    }

    Ok(())
}

fn validate_input(
    project: &str,
    environment: &str,
    names: &Vec<String>,
) -> Result<(), InputValidationError> {
    let name_is_valid = validate_project_name(project, false, false);

    if let Err(err) = name_is_valid {
        return Err(err);
    }

    let env_name_validation = validate_environment_name(environment, false, false);

    if let Err(err) = env_name_validation {
        return Err(err);
    }

    let names_valid = validate_secret_names(names);

    if let Err(err) = names_valid {
        return Err(err);
    }

    Ok(())
}
