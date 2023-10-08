use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFromat,
    models::{
        api_client::PostPatchRequestApiResponse,
        secrets::{GetSelectedSecretsPayload, Secret},
    },
    utils::{
        secrets::format_secrets,
        spinner::{self, request_spinner},
        validation::validate_project_name,
    },
};

pub struct HandleGetSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub keys: Vec<String>,
    pub format: SecretsFromat,
}

pub async fn handle_get_secrets(args: HandleGetSecretsArgs) -> Result<()> {
    let HandleGetSecretsArgs {
        token,
        project,
        environment,
        keys,
        format,
    } = args;

    // TODO: other validations
    let name_is_valid = validate_project_name(&project, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    debug!("listing secrets...:");

    let payload = GetSelectedSecretsPayload { keys: keys.clone() };

    let mut spinner = request_spinner();
    let res = secrets::get_selected(token, project, environment, payload).await;

    spinner.stop_and_persist("", "");

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();
    match res {
        PostPatchRequestApiResponse::Ok(data) => {
            if let Some(text) = data.text {
                debug!("{}", text);
                let secrets = serde_json::from_str::<Vec<Secret>>(&text);
                debug!("{:#?}", &secrets);

                match secrets {
                    Ok(secrets) => {
                        let secrets_not_found: Vec<_> = keys
                            .into_iter()
                            .filter(|k| secrets.iter().find(|s| s.key == *k).is_none())
                            .collect();

                        debug!("{:#?}", &secrets_not_found);

                        if !secrets_not_found.is_empty() {
                            eprintln!(
                                "{} {}",
                                "Secrets not found:".red(),
                                secrets_not_found.join(", ")
                            )
                        }

                        if !secrets.is_empty() {
                            eprintln!();
                            let print_string = format_secrets(secrets, &format);

                            if format == SecretsFromat::List {
                                print!("{}", print_string);
                            } else {
                                println!("{}", print_string);
                            }
                        }
                    }
                    Err(e) => {
                        error!("{}", e);
                        bail!("Something went wrong");
                    }
                }
            } else {
                debug!("No data");
                bail!("Something went wrong");
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            bail!("{}", e);
        }
    }

    Ok(())
}
