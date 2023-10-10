use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFromat,
    models::{api_client::GetRequestApiResponse, secrets::Secret},
    utils::{
        secrets::{format_secret_keys, format_secrets},
        spinner::request_spinner,
        validation::validate_project_environment,
    },
};

pub struct HandleListSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub format: SecretsFromat,
    pub only_keys: bool,
}

pub async fn handle_list_secrets(args: HandleListSecretsArgs) -> Result<()> {
    let HandleListSecretsArgs {
        token,
        project,
        environment: enironment,
        format,
        only_keys,
    } = args;

    let validation_res = validate_project_environment(&project, &enironment);

    if let Err(err) = validation_res {
        bail!(err);
    }

    debug!("listing secrets...:");

    let mut spinner = request_spinner();
    let res = secrets::list(token, project, enironment, only_keys).await;

    spinner.stop_and_persist("", "");

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => match only_keys {
            true => {
                let keys = serde_json::from_str::<Vec<String>>(&data.text);

                match keys {
                    Ok(keys) => {
                        let print_string = format_secret_keys(keys, &format);

                        println!("{}", print_string);
                    }
                    Err(e) => {
                        bail!("Something went wrong")
                    }
                }
            }
            false => {
                let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);

                match secrets {
                    Ok(secrets) => {
                        debug!("{:#?}", &secrets);

                        let print_string = format_secrets(secrets, &format);

                        if format == SecretsFromat::List {
                            print!("{}", print_string);
                        } else {
                            println!("{}", print_string);
                        }
                    }
                    Err(_) => {
                        bail!("Something went wrong")
                    }
                }
            }
        },
        GetRequestApiResponse::Err(e) => {
            bail!("{}", e);
        }
    }

    Ok(())
}
