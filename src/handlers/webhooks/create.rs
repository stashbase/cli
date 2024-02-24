use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::webhooks,
    models::{
        api_client::PostPatchRequestApiResponse,
        webhooks::{CreateWebhookPayload, CreateWebhookResponse},
    },
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

pub struct CreateWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    // payload
    pub url: String,
    pub description: Option<String>,
}

pub async fn handle_create_webhook(args: CreateWebhookArgs) -> Result<()> {
    let CreateWebhookArgs {
        api_key,
        project,
        environment,
        url,
        description,
    } = args;

    let args = webhooks::CreateArgs {
        api_key,
        project,
        environment,
        data: CreateWebhookPayload { url, description },
    };

    let mut spinner = request_spinner();

    let res = webhooks::create(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        bail!(format!("Error sending request: {}", err));
        // bail!(err);
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);

            match data.text {
                Some(text_data) => {
                    let webhook = serde_json::from_str::<CreateWebhookResponse>(&text_data);

                    match webhook {
                        Ok(webhook) => {
                            spinner.stop_with_message("🔥 Webhook created and enabled!");
                            println!("{} {}", "Id:", webhook.id);
                        }
                        Err(e) => {
                            spinner.stop_and_persist("", "");
                            debug!("Err: {}", e);
                            bail!("Something went wrong")
                        }
                    }
                }
                None => {
                    // NOTE: webhook creted but no id returned
                    spinner.stop_and_persist("", "");
                    bail!("Something went wrong")
                }
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
