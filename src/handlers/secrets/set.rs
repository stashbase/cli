use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{Secret, ValidateSecrets},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{interaction, secrets::format_secret_comment, separator, spinner::request_spinner},
};

pub struct HandleSetSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub values: Vec<String>,
    pub comment: Vec<String>,
    pub json_format: bool,
    pub silent: bool,
}

// NOTE: for now must have at least one value -> validate length
pub async fn handle_set_secrets(args: HandleSetSecretsArgs) -> Result<()> {
    let HandleSetSecretsArgs {
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

    debug!("{:#?}", comment);

    let name_value_pairs = separator::key_value(values);

    debug!("{:#?}", name_value_pairs);

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
    debug!("{:#?}", comment_pairs);

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
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }
                println!("{{}}");
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Secrets set.");
                }
            }
        }
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
