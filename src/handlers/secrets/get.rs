use std::collections::HashSet;

use anyhow::bail;
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    cmd::config::SecretsOutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        secrets::Secret,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        output::get_colored_json,
        secrets::format_secrets,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_names},
    },
};

pub struct HandleGetSecretsArgs {
    pub silent: bool,
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub names: Vec<String>,
    pub format: SecretsOutputFormat,
    pub expand_refs: bool,
}

pub async fn handle_get_secrets(args: HandleGetSecretsArgs) -> anyhow::Result<()> {
    let HandleGetSecretsArgs {
        silent,
        api_key,
        project,
        environment,
        names,
        format,
        expand_refs,
    } = args;

    let validation_res = validate_input(&project, &environment, &names);

    if let Err(e) = validation_res {
        let error_output = e.format_error_output(format == SecretsOutputFormat::Json)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    debug!("listing secrets...:");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = secrets::list(
        api_key,
        project,
        environment,
        false,
        Some(names.clone()),
        expand_refs,
    )
    .await;

    if let Some(mut spinner) = spinner {
        spinner.stop_and_persist("", "");
    }

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
                    if format == SecretsOutputFormat::Json {
                        let json_str = get_colored_json(&secrets).unwrap();
                        println!("{}", json_str);
                        return Ok(());
                    }

                    if secrets.len() < names.len() && !silent {
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
                    }
                }
                Err(e) => {
                    error!("{}", e);

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err =
                        error.format_error_output(format == SecretsOutputFormat::Json)?;

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            // bail!("{}", e);
            debug!("Error: {}", e);

            let error_output = e.format_error_output(format == SecretsOutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn validate_input(
    project: &str,
    environment: &str,
    names: &Vec<String>,
) -> Result<(), InputValidationError> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        return Err(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        return Err(err);
    }

    if names.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::NoNames);
        return Err(err);
    }

    let name_validation_res = validate_secret_names(names);

    if let Err(err) = name_validation_res {
        return Err(err);
    }

    Ok(())
}
