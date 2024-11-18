use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::Secret,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        duplicates::find_duplicates,
        interaction,
        secrets::format_secret_description,
        separator,
        spinner::request_spinner,
        validation::{
            validate_secret_description, validate_secret_names, validate_secret_values,
            validate_secrets_references,
        },
    },
};

pub struct HandleSetSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub values: Vec<String>,
    pub description: Vec<String>,
}

// NOTE: for now must have at least one value -> validate length
pub async fn handle_set_secrets(args: HandleSetSecretsArgs) -> Result<()> {
    let HandleSetSecretsArgs {
        api_key,
        project,
        environment,
        values,
        description,
    } = args;

    if values.is_empty() {
        let msg = format!("{} {}", "Input error:".red(), "no secrets to set");

        bail!("{}", msg);
    }

    debug!("{:#?}", description);

    let name_value_pairs = separator::key_value(values);

    debug!("{:#?}", name_value_pairs);

    if let Err(err) = name_value_pairs {
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let name_value_pairs = name_value_pairs.unwrap();

    // validate names
    let names: Vec<_> = name_value_pairs
        .iter()
        .map(|kv| format!("{}", kv.0))
        .collect();

    let names_valid = validate_secret_names(&names);

    if let Err(err) = names_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let duplicate_names = find_duplicates(&names);

    if !duplicate_names.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateNames(
            duplicate_names,
        ));

        bail!(err);
    }

    let values: Vec<_> = name_value_pairs.iter().map(|kv| kv.1.to_string()).collect();
    let values_valid = validate_secret_values(&values);

    if let Err(err) = values_valid {
        bail!(err);
    }

    let description_pairs = separator::key_value(description);
    debug!("{:#?}", description_pairs);

    if let Err(err) = description_pairs {
        // TODO: error
        bail!("{} {}", format!("Input error:").red(), err);
    }

    // OK
    let description_pairs = description_pairs.unwrap();

    let mut payload = Vec::new();

    for x in name_value_pairs {
        let description = description_pairs.iter().find(|d| d.0 == x.0);

        let secret = match description {
            Some((_, d_value)) => {
                let formatted_description = match d_value.is_empty() {
                    true => "".to_string(),
                    false => format_secret_description(&d_value.to_string(), true),
                };

                let description_validation_res =
                    validate_secret_description(&formatted_description);

                if let Err(err) = description_validation_res {
                    bail!(err);
                }

                Secret {
                    name: x.0,
                    value: x.1,
                    description: Some(formatted_description),
                }
            }
            None => Secret {
                name: x.0,
                value: x.1,
                description: None,
            },
        };

        payload.push(secret);
    }

    let references_validation = validate_secrets_references(&payload);

    if !references_validation.self_referenced_secrets.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::SelfReferences(
            references_validation.self_referenced_secrets,
        ));
        bail!(err);
    } else if !references_validation.invalid_format_references.is_empty() {
        let hint_str = references_validation
            .invalid_format_references
            .iter()
            .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
            .collect::<Vec<_>>()
            .join(", ");

        eprintln!("{}", format!("{}", "Input warning").yellow());

        eprintln!("- message: invalid secret references format");
        eprintln!("- secrets: {} \n", hint_str);

        let confirm = interaction::confirm_opt("Are you sure you want to continue?");

        if confirm.is_none() || (confirm.unwrap() == false) {
            return Ok(());
        }
    }

    let mut spinner = request_spinner();
    let res = secrets::set_sercrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            spinner.stop_with_message(&format!("{} {}", "✓".green(), "Secrets have been setted!"));
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
