use log::debug;

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{
        api_client::{ApiError, CustomError, PostPatchRequestApiResponse},
        webhooks::{TestWebhookResponse, UpdateWebhookStatusPayload},
    },
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

// TODO: return already enabled/disabled
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

    // TODO: return already enabled/disabled
    match res {
        PostPatchRequestApiResponse::Ok(res_data) => {
            if let Some(res_text) = res_data.text {
                let test_response = serde_json::from_str::<TestWebhookResponse>(&res_text);

                match test_response {
                    Ok(test_res) => match test_res {
                        TestWebhookResponse::Err(_) => todo!(),
                        TestWebhookResponse::Ok(_) => {}
                    },
                    Err(_) => todo!(),
                }
            } else {
                let msg = match enabled {
                    true => "✅ Webhook has been enabled!",
                    // false => "❌ Webhook has been disabled!",
                    false => "✅ Webhook has been disabled!",
                };

                // println!("Project has been deleted");
                spinner.stop_with_message(msg);
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
