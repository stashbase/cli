use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    handlers::secrets::pretty_print::print_secret_name_list,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{Secret, UpsertSecretsResponse, ValidateSecrets},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction, output::get_formatted_json_string, secrets::format_secret_comment, separator,
        spinner::request_spinner,
    },
};

pub struct HandleUpsertSecretsArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub values: Vec<String>,
    pub comment: Vec<String>,
    pub json_format: bool,
    pub silent: bool,
}

// NOTE: for now must have at least one value -> validate length
pub async fn handle_upsert_secrets(args: HandleUpsertSecretsArgs) -> Result<()> {
    let HandleUpsertSecretsArgs {
        api_key,
        project,
        environment,
        values,
        comment,
        json_format,
        silent,
    } = args;

    if values.is_empty() {
        let secrets_error = SecretsInputValidationError::NoSecretsToSet;
        let input_error = InputValidationError::Secrets(secrets_error);
        let error_output = input_error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    let name_value_pairs = separator::key_value(values);

    if let Err(_) = name_value_pairs {
        let error = InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);
        let error_output = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    let name_value_pairs = name_value_pairs.unwrap();

    let comment_pairs = separator::key_value(comment);

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
            eprintln!();
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
    let res = secrets::set_sercrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_str = err.format_error_output(json_format)?;
        bail!(error_str);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<UpsertSecretsResponse>(&text);

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

                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let created_count = data.created_secrets.len();
                        let updated_count = data.updated_secrets.len();

                        if silent {
                            println!("Created: {}", created_count);
                            println!("Updated: {}", updated_count);
                        } else {
                            match (created_count, updated_count) {
                                (0, 0) => println!("No secrets changed."),
                                _ => {
                                    println!("Created: {}", created_count);
                                    println!("Updated: {}", updated_count);
                                    print_secret_name_list(
                                        "Created secrets:",
                                        &data.created_secrets,
                                    );
                                    print_secret_name_list(
                                        "Updated secrets:",
                                        &data.updated_secrets,
                                    );
                                }
                            }
                        }
                    }
                    Err(_) => {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let error_str =
                            crate::models::api_client::OutputError::failed_to_deserialize_response_body()
                                .format_error_output(json_format)?;
                        bail!(error_str);
                    }
                }
            }
            None => {
                if json_format {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                    println!("{{}}");
                } else if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Secrets upserted.");
                }
            }
        },
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_str = e.format_error_output(json_format)?;
            bail!(error_str);
        }
    }

    Ok(())
}
