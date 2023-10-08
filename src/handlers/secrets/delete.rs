use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::PostPatchRequestApiResponse,
        secrets::{DeleteSecretsPayload, DeleteSecretsResponse},
    },
    utils::spinner::request_spinner,
};

pub struct HandleDeleteSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub keys: Vec<String>,
}

// ✓
pub async fn handle_delete_secrets(args: HandleDeleteSecretsArgs) -> Result<()> {
    let HandleDeleteSecretsArgs {
        token,
        project,
        environment,
        keys,
    } = args;

    // TODO: confirm
    // TODO: validation

    debug!("deleting secrets...:");

    let keys_len = keys.len();

    let payload = DeleteSecretsPayload { keys: keys.clone() };

    let mut spinner = request_spinner();

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
                    spinner.stop_with_message("🗑️ Selected secrets have been deleted!");
                }
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
