use log::debug;

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::api_client::RequestApiOptionResponse,
    utils::{interaction, spinner::request_spinner},
};

pub struct RotateWebhookSecretArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

impl From<RotateWebhookSecretArgs> for webhooks::RotateArgs {
    fn from(args: RotateWebhookSecretArgs) -> webhooks::RotateArgs {
        webhooks::RotateArgs {
            api_key: args.api_key,
            project: args.project,
            environment: args.environment,
            webhook_id: args.webhook_id,
        }
    }
}

pub async fn handle_rotate_webhook_secret(args: RotateWebhookSecretArgs) -> Result<()> {
    let json_format = args.json_format;
    let silent = args.silent;
    let force = args.force;

    if !force {
        let i = interaction::confirm_opt("Are you sure you want to rotate signing secret?");

        if i.is_none() || (i.unwrap() == false) {
            return Ok(());
        }
    }

    let args: webhooks::RotateArgs = args.into();
    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = webhooks::rotate_secret(args).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    // safe
    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }
                println!("{{}}");
            } else if !silent {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Webhook secret rotated.");
                }
                println!("Use 'webhooks get-secret <WEBHOOK_ID>' to fetch current signing secret.");
            } else if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }
        }
        RequestApiOptionResponse::Err(e) => {
            // eprintln!("{}", e);
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
