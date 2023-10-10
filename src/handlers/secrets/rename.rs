use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::PostPatchRequestApiResponse,
        secrets::{RenameSecretsResponse, RenamedSecret},
    },
    utils::{separator, spinner::request_spinner, validation::validate_secret_key_new_key},
};

pub struct HandleRenameSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub secrets: Vec<String>,
}

// TODO: input error - at least one item
pub async fn handle_rename_secrets(args: HandleRenameSecretsArgs) -> Result<()> {
    let HandleRenameSecretsArgs {
        token,
        project,
        environment,
        secrets,
    } = args;

    let key_value_pairs = separator::key_value(secrets);
    debug!("{:#?}", key_value_pairs);

    if let Err(err) = key_value_pairs {
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let key_value_pairs = key_value_pairs.unwrap();

    // NOTE: validation - keys + new keys
    let keys_valid = validate_secret_key_new_key(&key_value_pairs);

    if let Err(err) = keys_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let payload: Vec<_> = key_value_pairs
        .into_iter()
        .map(|k| RenamedSecret {
            key: k.0,
            new_key: k.1,
        })
        .collect();

    let mut spinner = request_spinner();

    let res = secrets::rename_secrets(token, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(res) => match res.text {
            Some(text) => {
                spinner.stop_and_persist("", "");

                let json_data = serde_json::from_str::<RenameSecretsResponse>(&text);
                debug!("{:#?}", json_data);

                match json_data {
                    Ok(data) => {
                        let secrets_not_found = data.not_found;
                        let not_found_len = secrets_not_found.len();

                        debug!("{:#?}", secrets_not_found);

                        let info_msg = format!(
                            "{} {}",
                            format!(
                                "{} {} {}",
                                "Secrets".red(),
                                format!("({})", not_found_len).red(),
                                "not found:".red()
                            ),
                            secrets_not_found.join(", ")
                        );

                        //
                        eprintln!("{}", info_msg);

                        let keys_len = payload.len();

                        if not_found_len < keys_len {
                            let renamed_len = keys_len - not_found_len;

                            let secrets_deleted: Vec<_> = payload
                                .into_iter()
                                .filter_map(|k| {
                                    if secrets_not_found
                                        .iter()
                                        .find(|s| **s == k.get_key())
                                        .is_some()
                                    {
                                        None
                                    } else {
                                        Some(k.key)
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
                                secrets_deleted.join(", ")
                            );

                            println!("{}", msg);
                        }
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
                        bail!("Something went wrong");
                    }
                }
            }
            None => {
                spinner.stop_with_message(&format!(
                    "{} {}",
                    "✓".green(),
                    "Selected secrets have been renamed!"
                ));
            }
        },
        PostPatchRequestApiResponse::Err(_) => todo!(),
    }

    Ok(())
}
