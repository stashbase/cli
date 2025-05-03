use std::collections::HashSet;

use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    cmd::config::SecretsOutputFormat,
    models::{
        api_client::GetRequestApiResponse,
        secrets::Secret,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        secrets::format_secrets,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_names},
    },
};

pub struct HandleGetSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub names: Vec<String>,
    pub format: SecretsOutputFormat,
    pub expand_refs: bool,
}

pub async fn handle_get_secrets(args: HandleGetSecretsArgs) -> Result<()> {
    let HandleGetSecretsArgs {
        api_key,
        project,
        environment,
        names,
        format,
        expand_refs,
    } = args;

    let validation_res = validate_input(&project, &environment, &names);

    if let Err(e) = validation_res {
        eprintln!();
        bail!(e);
    }

    debug!("listing secrets...:");

    let mut spinner = request_spinner();
    let res = secrets::list(
        api_key,
        project,
        environment,
        false,
        Some(names.clone()),
        expand_refs,
    )
    .await;

    spinner.stop_and_persist("", "");

    if let Err(err) = res {
        let error_output = err.format_error_output(format == SecretsOutputFormat::Json)?;
        bail!(error_output);
    }

    let res = res.unwrap();
    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);
            debug!("{:#?}", &secrets);

            match secrets {
                Ok(secrets) => {
                    if secrets.len() < names.len() {
                        let names_set: HashSet<String> = names.into_iter().collect();

                        let secrets_not_found: Vec<_> = names_set
                            .difference(&secrets.iter().map(|s| s.name.clone()).collect())
                            .cloned()
                            .collect();

                        if !secrets_not_found.is_empty() {
                            eprintln!(
                                "{} {}",
                                "Secrets not found:".red(),
                                secrets_not_found.join(", ")
                            )
                        }

                        if !secrets.is_empty() {
                            eprintln!();
                        }
                    }

                    if !secrets.is_empty() {
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
                    bail!("Something went wrong.");
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            // bail!("{}", e);
            debug!("Error: {}", e);
            eprintln!("");

            let error_output = e.format_error_output(format == SecretsOutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn validate_input(project: &str, environment: &str, names: &Vec<String>) -> Result<()> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        bail!(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        bail!(err);
    }

    if names.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::NoNames);
        bail!(err);
    }

    let name_validation_res = validate_secret_names(names);

    if let Err(err) = name_validation_res {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    Ok(())
}
