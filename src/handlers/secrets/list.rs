use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    cmd::config::SecretsOutputFormat,
    models::{
        api_client::GetRequestApiResponse,
        secrets::{Secret, SecretOptional},
    },
    utils::{
        secrets::{format_secret_keys, format_secrets},
        spinner::request_spinner,
        validation::{validate_project_environment, validate_secret_search},
    },
};

pub struct HandleListSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    // pub search: Option<String>,
    pub format: SecretsOutputFormat,
    pub only_keys: bool,
    pub expand_refs: bool,
}

pub async fn handle_list_secrets(args: HandleListSecretsArgs) -> Result<()> {
    let HandleListSecretsArgs {
        api_key,
        project,
        environment: enironment,
        format,
        only_keys,
        expand_refs,
    } = args;

    let validation_res = validate_project_environment(&project, &enironment, false);

    if let Err(err) = validation_res {
        bail!(err);
    }

    // if let Some(search) = &search {
    //     let search_validation_res = validate_secret_search(search);
    //
    //     if let Err(err) = search_validation_res {
    //         bail!(err);
    //     }
    // }

    debug!("listing secrets...:");

    let mut spinner = request_spinner();
    let res = secrets::list(api_key, project, enironment, only_keys, None, expand_refs).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => match only_keys {
            true => {
                let keys = serde_json::from_str::<Vec<SecretOptional>>(&data.text);

                match keys {
                    Ok(secrets) => {
                        if secrets.is_empty() {
                            spinner.stop_with_message("No secrets found");
                        } else {
                            let keys = secrets.into_iter().map(|s| s.key).collect::<Vec<_>>();
                            let print_string = format_secret_keys(keys, &format);

                            spinner.stop_and_persist("", "");

                            println!("{}", print_string);
                        }
                    }
                    Err(e) => {
                        debug!("{}", e);
                        spinner.stop_and_persist("", "");
                        bail!("Something went wrong")
                    }
                }
            }
            false => {
                let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);

                match secrets {
                    Ok(secrets) => {
                        debug!("{:#?}", &secrets);

                        if secrets.is_empty() {
                            spinner.stop_with_message("No secrets found");
                        } else {
                            spinner.stop_and_persist("", "");
                            let print_string = format_secrets(secrets, &format);

                            println!("{}", print_string);

                            //     if format == SecretsFromat::List {
                            //     } else {
                            //         println!("{}", print_string);
                            //     }
                        }
                    }
                    Err(_) => {
                        spinner.stop_and_persist("", "");
                        bail!("Something went wrong")
                    }
                }
            }
        },
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
