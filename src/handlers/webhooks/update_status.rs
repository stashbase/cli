use log::debug;

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{api_client::RequestApiOptionResponse, webhooks::UpdateWebhookStatusPayload},
    utils::{interaction, spinner::request_spinner},
};

pub struct UpdateWebhookStatusArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub enabled: bool,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

impl From<UpdateWebhookStatusArgs> for webhooks::UpdateStatusArgs {
    fn from(args: UpdateWebhookStatusArgs) -> webhooks::UpdateStatusArgs {
        webhooks::UpdateStatusArgs {
            api_key: args.api_key,
            project: args.project,
            environment: args.environment,
            webhook_id: args.webhook_id,
            data: UpdateWebhookStatusPayload {
                enabled: args.enabled,
            },
        }
    }
}

pub async fn handle_update_webhook_status(args: UpdateWebhookStatusArgs) -> Result<()> {
    let enabled = args.enabled;
    let json_format = args.json_format;
    let silent = args.silent;
    let force = args.force;

    if !force {
        let i = interaction::confirm_opt("Are you sure?");

        if i.is_none() || (i.unwrap() == false) {
            return Ok(());
        }
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let req_args: webhooks::UpdateStatusArgs = args.into();

    let res = webhooks::update_status(req_args).await;

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
            } else {
                if let Some(mut spinner) = spinner {
                    let msg = match enabled {
                        true => "Webhook enabled.",
                        false => "Webhook disabled.",
                    };
                    spinner.stop_with_message(msg);
                }
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
