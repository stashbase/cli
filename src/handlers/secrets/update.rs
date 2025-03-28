use std::collections::HashMap;

use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{
            Secret, SecretPropertiesToUpdate, UpdateSecretsPayload, UpdateSecretsResponse,
            UpdatedSecret, ValidateSecrets, ValidateUpdateSecrets,
        },
    },
    utils::{interaction, secrets::format_secret_comment, separator, spinner::request_spinner},
};

pub struct HandleUpdateSecretsArgs {
    pub json_format: bool,
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub renames: Vec<String>,
    pub values: Vec<String>,
    pub comment: Vec<String>,
}

pub async fn handle_update_secrets(args: HandleUpdateSecretsArgs) -> Result<()> {
    let HandleUpdateSecretsArgs {
        api_key,
        project,
        environment,
        renames,
        comment,
        values,
        json_format,
    } = args;

    if values.is_empty() && renames.is_empty() && comment.is_empty() {
        let msg = format!(
            "{} {}",
            "Input error:".red(),
            "no values, renames or comments to update provided"
        );

        bail!("{}", msg);
    }

    // name -> ({new_name, value, comment})
    let mut secret_updates: HashMap<String, SecretPropertiesToUpdate> = HashMap::new();

    // process new values
    if !values.is_empty() {
        let name_value_pairs = separator::key_value(values);

        if let Err(err) = name_value_pairs {
            bail!("{} {}", format!("Input error:").red(), err);
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

    // process renames
    if !renames.is_empty() {
        let name_value_pairs = separator::key_value(renames);

        if let Err(err) = name_value_pairs {
            bail!("{} {}", format!("Input error:").red(), err);
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

        if let Err(err) = name_value_pairs {
            bail!("{} {}", format!("Input error:").red(), err);
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

    // TODO: validate input

    // Convert inputs into UpdateSecretsPayload
    let payload: Vec<UpdatedSecret> = secret_updates
        .into_iter()
        .map(|(name, properties)| UpdatedSecret {
            name,
            value: properties.value,
            new_name: properties.new_name,
            comment: properties.comment,
        })
        .collect();

    if let Err(err) = payload.validate() {
        bail!(err);
    }

    let mut spinner = request_spinner();
    let res = secrets::update_secrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
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
                            spinner.stop_and_persist("", "");

                            let value = serde_json::to_value(data).unwrap();
                            let pretty = to_colored_json_auto(&value).unwrap();

                            println!("{}", pretty);
                        } else {
                            let updated_count = data.updated_count;
                            let not_found_secrets = data.not_found_secrets;

                            spinner.stop_and_persist("", "");

                            if updated_count > 0 {
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
                                        format!("({}): ", updated_count).green(),
                                    ),
                                    secrets_updated
                                        .iter()
                                        .map(|s| s.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );

                                println!("{}", msg);
                            } else {
                                let msg = format!("No secrets updated (no changes)");
                                println!("{}", msg);
                            }

                            if not_found_secrets.len() > 0 {
                                let info_msg = format!(
                                    "{} {}",
                                    format!(
                                        "{} {} {}",
                                        "Secrets".red(),
                                        "not found".red(),
                                        format!("({}): ", not_found_secrets.len()).red(),
                                    ),
                                    not_found_secrets.join(", ")
                                );

                                //
                                println!("{}", info_msg);
                            }
                        }
                    }
                    Err(err) => {
                        spinner.stop_with_message(&format!("{}", err));
                    }
                }
            }
            None => {
                spinner.stop_with_message(&format!("{}", "Something went wrong"));
            }
        },
        RequestApiOptionResponse::Err(err) => {
            spinner.stop_and_persist("", "");
            debug!("Error: {:#?}", &err);
            bail!(err);
        }
    }

    Ok(())
}
