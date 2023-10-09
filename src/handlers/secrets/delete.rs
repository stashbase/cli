use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::{DeleteRequestApiResponse, PostPatchRequestApiResponse},
        secrets::{DeleteAllSecretsResponse, DeleteSecretsPayload, DeleteSecretsResponse},
    },
    utils::{interaction, spinner::request_spinner},
};

pub struct HandleDeleteSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub keys: Vec<String>,
    pub delete_all: bool,
}

// ✓
pub async fn handle_delete_secrets(args: HandleDeleteSecretsArgs) -> Result<()> {
    let HandleDeleteSecretsArgs {
        token,
        project,
        environment,
        delete_all,
        keys,
    } = args;

    // TODO: confirm
    // TODO: validation

    if delete_all {
        eprintln!(
            "{}",
            "All secrets in selected environment will be deleted".red()
        );
    }

    let i = interaction::confirm_opt("Are you sure you want to continue?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }
    debug!("deleting secrets...:");

    let mut spinner = request_spinner();

    match delete_all {
        true => {
            let res = secrets::delete_all(token, project, environment).await;

            if let Err(err) = res {
                spinner.stop_and_persist("", "");
                error!("{:#?}", &err);
                bail!(err);
            }

            let res = res.unwrap();

            match res {
                DeleteRequestApiResponse::Ok(res) => {
                    match res.text {
                        Some(text) => {
                            //
                            let json_data = serde_json::from_str::<DeleteAllSecretsResponse>(&text);

                            match json_data {
                                Ok(_) => {
                                    spinner.stop_with_message("No secrets to delete");
                                }
                                Err(e) => {
                                    error!("{}", e);
                                    bail!("Something went wrong");
                                }
                            }
                        }
                        None => spinner.stop_with_message(&format!(
                            "{} {}",
                            "✓".green(),
                            "All secrets have been deleted"
                        )),
                    }
                }
                DeleteRequestApiResponse::Err(e) => {
                    spinner.stop_with_message(&format!("\n{}", e));
                }
            }
        }
        false => {
            let payload = DeleteSecretsPayload { keys: keys.clone() };

            let res = secrets::delete(token, project, environment, payload).await;

            if let Err(err) = res {
                spinner.stop_and_persist("", "");
                error!("{:#?}", &err);
                bail!(err);
            }

            let res = res.unwrap();

            match res {
                PostPatchRequestApiResponse::Ok(res) => {
                    // all deleted
                    match res.text {
                        Some(text) => {
                            spinner.stop_and_persist("", "");

                            let json_data = serde_json::from_str::<DeleteSecretsResponse>(&text);
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

                                    let keys_len = keys.len();

                                    if not_found_len < keys_len {
                                        let deleted_len = keys_len - not_found_len;

                                        let secrets_deleted: Vec<_> = keys
                                            .into_iter()
                                            .filter(|k| {
                                                secrets_not_found.iter().find(|s| *s == k).is_none()
                                            })
                                            .collect();

                                        let msg = format!(
                                            "{} {}",
                                            format!(
                                                "{} {} {}",
                                                "Secrets".green(),
                                                format!("({})", deleted_len).green(),
                                                "deleted:".green()
                                            ),
                                            secrets_deleted.join(", ")
                                        );

                                        println!("{}", msg);
                                    }
                                }
                                Err(e) => {
                                    error!("{}", e);
                                }
                            }
                        }
                        None => {
                            // spinner.stop_with_message("🗑️ Selected secrets have been deleted!");
                            spinner.stop_with_message(&format!(
                                "{} {}",
                                "✓".green(),
                                "Selected secrets have been delete!"
                            ));
                        }
                    }
                }
                PostPatchRequestApiResponse::Err(e) => {
                    spinner.stop_with_message(&format!("\n{}", e));
                }
            }
        }
    }

    Ok(())
}
