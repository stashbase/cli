use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    cmd::config::SecretsOutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        secrets::{Secret, SecretOptional},
    },
    utils::{
        output::get_colored_json,
        secrets::{format_secret_names, format_secrets},
        spinner::request_spinner,
    },
};

pub struct HandleListSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    // pub search: Option<String>,
    pub format: SecretsOutputFormat,
    pub only_names: bool,
    pub expand_refs: bool,
}

pub async fn handle_list_secrets(args: HandleListSecretsArgs) -> Result<()> {
    let HandleListSecretsArgs {
        api_key,
        project,
        environment: enironment,
        format,
        only_names,
        expand_refs,
    } = args;

    // if let Some(search) = &search {
    //     let search_validation_res = validate_secret_search(search);
    //
    //     if let Err(err) = search_validation_res {
    //         bail!(err);
    //     }
    // }

    debug!("listing secrets...:");

    let mut spinner = request_spinner();
    let res = secrets::list(api_key, project, enironment, only_names, None, expand_refs).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(format == SecretsOutputFormat::Json)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => match only_names {
            true => {
                let names = serde_json::from_str::<Vec<SecretOptional>>(&data.text);

                match names {
                    Ok(secrets) => {
                        if secrets.is_empty() {
                            if format == SecretsOutputFormat::Json {
                                let json_str = get_colored_json(&secrets).unwrap();

                                spinner.stop_and_persist("", "");
                                println!("{}", json_str);
                            } else {
                                spinner.stop_with_message("No secrets found.");
                            }
                        } else {
                            let names = secrets.into_iter().map(|s| s.name).collect::<Vec<_>>();
                            let print_string = format_secret_names(names, &format);

                            spinner.stop_and_persist("", "");

                            println!("{}", print_string);
                        }
                    }
                    Err(_) => {
                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err =
                            error.format_error_output(format == SecretsOutputFormat::Json)?;

                        spinner.stop_and_persist("", "");
                        bail!(formatted_err);
                    }
                }
            }
            false => {
                let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);

                match secrets {
                    Ok(secrets) => {
                        debug!("{:#?}", &secrets);

                        if secrets.is_empty() {
                            if format == SecretsOutputFormat::Json {
                                let json_str = get_colored_json(&secrets).unwrap();

                                spinner.stop_and_persist("", "");
                                println!("{}", json_str);
                            } else {
                                spinner.stop_with_message("No secrets found.");
                            }
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

                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err =
                            error.format_error_output(format == SecretsOutputFormat::Json)?;

                        bail!(formatted_err);
                    }
                }
            }
        },
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(format == SecretsOutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}
