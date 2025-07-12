use std::collections::HashMap;

use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        secrets::{
            SecretPropertiesToUpdate, UpdateSecretsPayload, UpdateSecretsResponse, UpdatedSecret,
            ValidateUpdateSecrets,
        },
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{output::get_colored_json, separator, spinner::request_spinner},
};

pub struct HandleUpdateSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub new_names: Vec<String>,
    pub values: Vec<String>,
    pub comment: Vec<String>,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_update_secrets(args: HandleUpdateSecretsArgs) -> Result<()> {
    let HandleUpdateSecretsArgs {
        api_key,
        project,
        environment,
        new_names,
        comment,
        values,
        json_format,
        silent,
    } = args;

    if values.is_empty() && new_names.is_empty() && comment.is_empty() {
        let error = InputValidationError::Secrets(SecretsInputValidationError::NoUpdatesProvided);
        let error_output = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    // name -> {new_name, value, comment}
    let mut secret_updates: HashMap<String, SecretPropertiesToUpdate> = HashMap::new();

    // process new values
    if !values.is_empty() {
        let name_value_pairs = separator::key_value(values);

        if let Err(err) = name_value_pairs {
            let error =
                InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);

            let error_output = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        for (name, value) in name_value_pairs.unwrap() {
            let existing_secret = secret_updates.get_mut(&name);

            if let Some(secret) = existing_secret {
                secret.value = Some(value);
            } else {
                let properties = SecretPropertiesToUpdate {
                    new_name: None,
                    value: Some(value),
                    comment: None,
                };

                secret_updates.insert(name, properties);
            }
        }
    }

    // process new names (renamed secrets)
    if !new_names.is_empty() {
        let name_value_pairs = separator::key_value(new_names);

        if let Err(err) = name_value_pairs {
            let error =
                InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);

            let error_output = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        for (name, new_name) in name_value_pairs.unwrap() {
            let existing_secret = secret_updates.get_mut(&name);

            if let Some(secret) = existing_secret {
                secret.new_name = Some(new_name);
            } else {
                let properties = SecretPropertiesToUpdate {
                    new_name: Some(new_name),
                    value: None,
                    comment: None,
                };

                secret_updates.insert(name, properties);
            }
        }
    }

    // process comments
    if !comment.is_empty() {
        let name_value_pairs = separator::key_value(comment);

        if let Err(_) = name_value_pairs {
            let error =
                InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);

            let error_output = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        for (name, comment) in name_value_pairs.unwrap() {
            let existing_secret = secret_updates.get_mut(&name);

            if let Some(secret) = existing_secret {
                secret.comment = Some(comment);
            } else {
                let properties = SecretPropertiesToUpdate {
                    new_name: None,
                    value: None,
                    comment: Some(comment),
                };

                secret_updates.insert(name, properties);
            }
        }
    }

    // Convert inputs into UpdateSecretsPayload
    let payload: UpdateSecretsPayload = secret_updates
        .into_iter()
        .map(|(name, properties)| UpdatedSecret {
            name,
            value: properties.value,
            new_name: properties.new_name,
            comment: properties.comment,
        })
        .collect();

    if let Err(err) = payload.validate() {
        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };
    let res = secrets::update_secrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();
    debug!("Response: {:#?}", &res);

    match res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<UpdateSecretsResponse>(&text);

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

                        let updated_count = data.updated_count;
                        let not_found_secrets = data.not_found_secrets;

                        if updated_count > 0 {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }

                            if !silent {
                                let secrets_updated: Vec<_> = payload
                                    .into_iter()
                                    .filter(|k| {
                                        not_found_secrets.iter().find(|s| *s == &k.name).is_none()
                                    })
                                    .collect();

                                let msg = format!(
                                    "{} {}",
                                    format!(
                                        "{} {} {}",
                                        "Secrets".green(),
                                        "updated".green(),
                                        format!("({}):", updated_count).green(),
                                    ),
                                    secrets_updated
                                        .iter()
                                        .map(|s| s.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );

                                println!("{}", msg);
                            }
                        } else {
                            if updated_count == 0 && not_found_secrets.len() == 0 {
                                if let Some(mut spinner) = spinner {
                                    if !silent {
                                        spinner.stop_and_persist(
                                            "No secrets updated (no changes).",
                                            "",
                                        );
                                    } else {
                                        spinner.stop_and_persist("", "");
                                    }
                                } else if !silent {
                                    println!("No secrets updated (no changes).");
                                }
                            } else {
                                if let Some(mut spinner) = spinner {
                                    spinner.stop_and_persist("", "");
                                }
                                if !silent {
                                    let msg = format!("No secrets updated (no changes).");
                                    println!("{}", msg);
                                }
                            }
                        }

                        if not_found_secrets.len() > 0 && !silent {
                            let info_msg = format!(
                                "{} {}",
                                format!(
                                    "{} {} {}",
                                    "Secrets".red(),
                                    "not found".red(),
                                    format!("({}):", not_found_secrets.len()).red(),
                                ),
                                not_found_secrets.join(", ")
                            );

                            println!("{}", info_msg);
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
        },
        RequestApiOptionResponse::Err(err) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }
            debug!("Error: {:#?}", &err);

            let error_output = err.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
