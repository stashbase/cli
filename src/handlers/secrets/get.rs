use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    cmd::config::SecretsOutputFormat,
    models::{
        api_client::PostPatchRequestApiResponse,
        secrets::{GetSelectedSecretsPayload, Secret},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        secrets::format_secrets,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_keys},
    },
};

pub struct HandleGetSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub keys: Vec<String>,
    pub format: SecretsOutputFormat,
    pub replace_refs: bool,
}

pub async fn handle_get_secrets(args: HandleGetSecretsArgs) -> Result<()> {
    let HandleGetSecretsArgs {
        api_key,
        project,
        environment,
        keys,
        format,
        replace_refs,
    } = args;

    let validation_res = validate_input(&project, &environment, &keys);

    if let Err(e) = validation_res {
        bail!(e);
    }

    debug!("listing secrets...:");

    let payload = GetSelectedSecretsPayload {
        keys: keys.clone(),
        replace_refs,
    };

    let mut spinner = request_spinner();
    let res = secrets::get_selected(api_key, project, environment, &payload).await;

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
                            if !secrets_not_found.is_empty() {
                                eprintln!();
                            }

                            let print_string = format_secrets(secrets, &format);
                            println!("{}", print_string);

                            // if format == SecretsFromat::List {
                            //     print!("{}", print_string);
                            // } else {
                            // }
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
            // bail!("{}", e);
            debug!("Error: {}", e);
            eprintln!("{}", e);
        }
    }

    Ok(())
}

fn validate_input(project: &str, environment: &str, keys: &Vec<String>) -> Result<()> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        bail!(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        bail!(err);
    }

    if keys.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::NoKeys);
        bail!(err);
    }

    let key_validation_res = validate_secret_keys(keys);

    if let Err(err) = key_validation_res {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    Ok(())
}
