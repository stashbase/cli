use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    handlers::secrets::pretty_print::print_secret_name_list,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        secrets::{CreateSecretsResponse, Secret, ValidateSecrets},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction, output::get_formatted_json_string, secrets::format_secret_comment, separator,
        spinner::request_spinner,
    },
};

pub struct HandleCreateSecretsArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub values: Vec<String>,
    pub comments: Vec<String>,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_create_secrets(args: HandleCreateSecretsArgs) -> Result<()> {
    let HandleCreateSecretsArgs {
        api_key,
        project,
        environment,
        values,
        comments,
        json_format,
        silent,
    } = args;

    if values.is_empty() {
        let error = InputValidationError::Secrets(SecretsInputValidationError::NoSecretsToCreate);
        let error_output = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    let name_value_pairs = separator::key_value(values);

    if let Err(_) = name_value_pairs {
        if !silent {
            eprintln!();
        }

        let error = InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);
        let error_output = error.format_error_output(json_format)?;

        bail!(error_output);
    }

    let name_value_pairs = name_value_pairs.unwrap();

    let comment_pairs = separator::key_value(comments);

    if let Err(_) = comment_pairs {
        let error = InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);
        let error_output = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    // OK
    let comment_pairs = comment_pairs.unwrap();
    let mut payload = Vec::new();

    for x in name_value_pairs {
        let comment = comment_pairs.iter().find(|d| d.0 == x.0);

        let secret = match comment {
            Some((_, c_value)) => {
                let formatted_comment = match c_value.is_empty() {
                    true => "".to_string(),
                    false => format_secret_comment(&c_value.to_string(), true),
                };

                Secret {
                    name: x.0,
                    value: x.1,
                    comment: Some(formatted_comment),
                }
            }
            None => Secret {
                name: x.0,
                value: x.1,
                comment: None,
            },
        };

        payload.push(secret);
    }

    if let Err(err) = payload.validate() {
        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    if !silent {
        let reference_warnings = payload.get_reference_warnings();

        if !reference_warnings.is_empty() {
            eprint!("{}", reference_warnings);

            let confirm = interaction::confirm_opt("Are you sure you want to continue?");

            if confirm.is_none() || (confirm.unwrap() == false) {
                return Ok(());
            }
        }
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = secrets::create_secrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<CreateSecretsResponse>(&text);

                match json_data {
                    Ok(data) => {
                        if json_format {
                            let json_str = get_formatted_json_string(&data, true).unwrap();

                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }

                            println!("{}", json_str);
                            return Ok(());
                        }

                        let created_count = data.created_count;
                        let existing_secrets = data.existing_secrets;
                        let created_secrets: Vec<String> = payload
                            .iter()
                            .filter(|k| existing_secrets.iter().find(|s| *s == &k.name).is_none())
                            .map(|s| s.name.clone())
                            .collect();

                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        if silent {
                            println!("Created: {}", created_count);
                            if !existing_secrets.is_empty() {
                                println!("Already exist: {}", existing_secrets.join(", "));
                            }
                        } else {
                            if created_count == 0 {
                                println!("No secrets created.");
                            } else {
                                println!("Created: {}", created_count);
                            }

                            if !existing_secrets.is_empty() {
                                println!("Already existing: {}", existing_secrets.len());
                            }

                            if !created_secrets.is_empty() {
                                print_secret_name_list("Created secrets:", &created_secrets);
                            }

                            if !existing_secrets.is_empty() {
                                print_secret_name_list(
                                    "Already existing secrets:",
                                    &existing_secrets,
                                );
                            }
                        }
                    }
                    Err(_) => {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        bail!(formatted_err);
                    }
                }
            }
            None => {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let error = OutputError::failed_to_deserialize_response_body();
                let formatted_err = error.format_error_output(json_format)?;

                eprintln!();
                bail!(formatted_err);
            }
        },
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);

            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
