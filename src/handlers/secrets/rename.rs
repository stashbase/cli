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
        separator,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_keys},
    },
};

pub struct HandleRenameSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub secrets: Vec<String>,
}

// TODO: input error - at least one item
pub async fn handle_rename_secrets(args: HandleRenameSecretsArgs) -> Result<()> {
    let HandleRenameSecretsArgs {
        api_key,
        project,
        environment,
        secrets,
    } = args;

    if secrets.is_empty() {
        let msg = format!(
            "{} {}",
            "Input error:".red(),
            "no secrets to rename provided"
        );

        bail!("{}", msg);
    }

    let key_value_pairs = separator::key_value(secrets);
    debug!("{:#?}", key_value_pairs);

    if let Err(err) = key_value_pairs {
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let key_value_pairs = key_value_pairs.unwrap();

    let validation_res = validate_input(&project, &environment, &key_value_pairs);

    if let Err(e) = validation_res {
        bail!("{}", e);
    }

    let new_keys = key_value_pairs
        .iter()
        .map(|k| k.1.to_string())
        .collect::<Vec<_>>();

    let duplicate_new_keys = duplicates::find_duplicates(&new_keys);

    if !duplicate_new_keys.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateNewKeys(
            duplicate_new_keys,
        ));

        bail!("{}", err);
    }

    // OK

    let payload: Vec<_> = key_value_pairs
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
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<RenameSecretsResponse>(&text);
                debug!("{:#?}", json_data);

                match json_data {
                    Ok(data) => {
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

                            let keys_len = payload.len();

                            if not_found_len < keys_len {
                                let renamed_len = keys_len - not_found_len;

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
                            spinner.stop_with_message(&format!(
                                "{} {}",
                                "✓".green(),
                                "Selected secrets have been renamed!"
                            ));
                        }
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
                        bail!("Something went wrong");
                    }
                }
            }
            None => {
                bail!("Something went wrong");
            }
        },
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}

fn validate_input(
    project: &str,
    environment: &str,
    key_value_pairs: &Vec<(String, String)>,
) -> Result<()> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        bail!(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        bail!(err);
    }

    let old_keys = key_value_pairs
        .iter()
        .map(|k| k.0.to_string())
        .collect::<Vec<_>>();

    let valid_old_keys = validate_secret_keys(&old_keys);

    if let Err(err) = valid_old_keys {
        bail!(err);
    }

    let new_keys = key_value_pairs
        .iter()
        .map(|k| k.1.to_string())
        .collect::<Vec<_>>();

    let valid_new_keys = validate_secret_keys(&new_keys);

    if let Err(err) = valid_new_keys {
        bail!(err);
    }

    let duplicate_keys = find_duplicates(&old_keys);

    if !duplicate_keys.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateKeys(
            duplicate_keys,
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
