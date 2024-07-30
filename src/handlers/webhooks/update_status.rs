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
    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let mut spinner = request_spinner();

    let enabled = args.enabled;
    let req_args: webhooks::UpdateStatusArgs = args.into();

    let res = webhooks::update_status(req_args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    // safe
    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            let msg = match enabled {
                true => "✅ Webhook has been enabled!",
                // false => "❌ Webhook has been disabled!",
                false => "✅ Webhook has been disabled!",
            };

            // println!("Project has been deleted");
            spinner.stop_with_message(msg);
        }
        RequestApiOptionResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
