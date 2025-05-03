use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{RenameSecretsResponse, RenamedSecret},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        duplicates::{self, find_duplicates},
        output::get_colored_json,
        separator,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_names},
    },
};

pub struct HandleRenameSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub secrets: Vec<String>,
    pub json_format: bool,
}

// TODO: input error - at least one item
pub async fn handle_rename_secrets(args: HandleRenameSecretsArgs) -> Result<()> {
    let HandleRenameSecretsArgs {
        api_key,
        project,
        environment,
        secrets,
        json_format,
    } = args;

    if secrets.is_empty() {
        let msg = format!(
            "{} {}",
            "Input error:".red(),
            "No secrets to rename provided."
        );

        eprintln!();
        bail!(msg);
    }

    let name_value_pairs = separator::key_value(secrets);
    debug!("{:#?}", name_value_pairs);

    if let Err(err) = name_value_pairs {
        eprintln!();
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let name_value_pairs = name_value_pairs.unwrap();

    let validation_res = validate_input(&project, &environment, &name_value_pairs);

    if let Err(e) = validation_res {
        eprintln!();
        bail!(e);
    }

    let new_names = name_value_pairs
        .iter()
        .map(|k| k.1.to_string())
        .collect::<Vec<_>>();

    let duplicate_new_names = duplicates::find_duplicates(&new_names);

    if !duplicate_new_names.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateNewNames(
            duplicate_new_names,
        ));

        eprintln!();
        bail!(err);
    }

    // OK

    let payload: Vec<_> = name_value_pairs
        .into_iter()
        .map(|k| RenamedSecret {
            name: k.0,
            new_name: k.1,
        })
        .collect();

    let mut spinner = request_spinner();

    let res = secrets::rename_secrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<RenameSecretsResponse>(&text);
                debug!("{:#?}", json_data);

                match json_data {
                    Ok(data) => {
                        if json_format {
                            let json_str = get_colored_json(&data).unwrap();

                            spinner.stop_and_persist("", "");
                            println!("{}", json_str);

                            return Ok(());
                        }

                        let not_found_secrets = data.not_found_secrets;
                        let not_found_len = not_found_secrets.len();

                        if not_found_len > 0 {
                            spinner.stop_and_persist("", "");

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

                            //
                            eprintln!("{}", info_msg);

                            let names_len = payload.len();

                            if not_found_len < names_len {
                                let renamed_len = names_len - not_found_len;

                                let secrets_renamed: Vec<_> = payload
                                    .into_iter()
                                    .filter_map(|k| {
                                        if not_found_secrets
                                            .iter()
                                            .find(|s| **s == k.get_name())
                                            .is_some()
                                        {
                                            None
                                        } else {
                                            Some(k.name)
                                        }
                                    })
                                    .collect();

                                let msg = format!(
                                    "{} {}",
                                    format!(
                                        "{} {} {}",
                                        "Secrets".green(),
                                        format!("({})", renamed_len).green(),
                                        "renamed:".green()
                                    ),
                                    secrets_renamed.join(", ")
                                );

                                println!("{}", msg);
                            }
                        } else {
                            spinner.stop_with_message("Selected secrets renamed.");
                        }
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
                        bail!("Something went wrong.");
                    }
                }
            }
            None => {
                bail!("Something went wrong.");
            }
        },
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_with_message(&format!("{}", e));

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn validate_input(
    project: &str,
    environment: &str,
    name_value_pairs: &Vec<(String, String)>,
) -> Result<()> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        bail!(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        bail!(err);
    }

    let old_names = name_value_pairs
        .iter()
        .map(|k| k.0.to_string())
        .collect::<Vec<_>>();

    let valid_old_names = validate_secret_names(&old_names);

    if let Err(err) = valid_old_names {
        bail!(err);
    }

    let new_names = name_value_pairs
        .iter()
        .map(|k| k.1.to_string())
        .collect::<Vec<_>>();

    let valid_new_names = validate_secret_names(&new_names);

    if let Err(err) = valid_new_names {
        bail!(err);
    }

    let duplicate_names = find_duplicates(&old_names);

    if !duplicate_names.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateNames(
            duplicate_names,
        ));

        bail!(err);
    }

    // let keys_valid_res = validate_secret_key_new_key(&key_value_pairs);
    //
    // if let Err(err) = keys_valid_res {
    //     debug!("Error: {:#?}", &err);
    //     bail!(err);
    // }

    Ok(())
}
