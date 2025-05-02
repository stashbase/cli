use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{Secret, ValidateSecrets},
    },
    utils::{interaction, secrets::format_secret_comment, separator, spinner::request_spinner},
};

pub struct HandleSetSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub values: Vec<String>,
    pub comment: Vec<String>,
}

// NOTE: for now must have at least one value -> validate length
pub async fn handle_set_secrets(args: HandleSetSecretsArgs) -> Result<()> {
    let HandleSetSecretsArgs {
        api_key,
        project,
        environment,
        values,
        comment,
    } = args;

    if values.is_empty() {
        let msg = format!("{} {}", "Input error:".red(), "no secrets to set");

        bail!("{}", msg);
    }

    debug!("{:#?}", comment);

    let name_value_pairs = separator::key_value(values);

    debug!("{:#?}", name_value_pairs);

    if let Err(err) = name_value_pairs {
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let name_value_pairs = name_value_pairs.unwrap();

    let comment_pairs = separator::key_value(comment);
    debug!("{:#?}", comment_pairs);

    if let Err(err) = comment_pairs {
        // TODO: error
        bail!("{} {}", format!("Input error:").red(), err);
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
        bail!(err);
    }

    let reference_warnings = payload.get_reference_warnings();

    if !reference_warnings.is_empty() {
        eprint!("{}", reference_warnings);

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
            spinner.stop_with_message("Secrets set.");
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
