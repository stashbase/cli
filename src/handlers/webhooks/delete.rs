use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::webhooks,
    models::api_client::DeleteRequestApiResponse,
    utils::{interaction, spinner::request_spinner},
};

pub struct DeleteWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_delete_webhook(args: DeleteWebhookArgs) -> Result<()> {
    let DeleteWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
        json_format,
        silent,
    } = args;

    // confirmation
    if !silent {
        eprintln!("{}", "Do you really want to delete this webhook?".red());

        let i = interaction::input("Type 'DELETE' to confirm.");

        if i != "DELETE" {
            println!("Input does not match, action aborted.");
            return Ok(());
        }
    }

    let args = webhooks::DeleteArgs {
        api_key,
        project,
        environment,
        webhook_id,
    };

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = webhooks::delete(args).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    debug!("{:#?}", &res);

    match res {
        DeleteRequestApiResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }
                println!("{{}}");
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Webhook deleted.");
                }
            }
        }
        DeleteRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
