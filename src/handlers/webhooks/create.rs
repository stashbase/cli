use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::webhooks,
    models::{
        api_client::RequestApiOptionResponse,
        webhooks::{CreateWebhookPayload, CreateWebhookResponse},
    },
    utils::spinner::request_spinner,
};

pub struct CreateWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub return_secret: bool,
    pub enable: bool,
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
        return_secret,
        enable,
    } = args;

    let args = webhooks::CreateArgs {
        api_key,
        project,
        environment,
        return_secret,
        data: CreateWebhookPayload {
            url,
            description,
            enabled: enable,
        },
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
        RequestApiOptionResponse::Ok(data) => {
            debug!("{:#?}", &data.text);

            match data.text {
                Some(text_data) => {
                    let webhook = serde_json::from_str::<CreateWebhookResponse>(&text_data);

                    match webhook {
                        Ok(webhook) => {
                            let msg = match enable {
                                true => "🔥 Webhook created and enabled!",
                                false => "🔥 Webhook created!",
                            };

                            spinner.stop_with_message(msg);
                            eprint!("Id: ");
                            print!("{}\n", webhook.id);

                            eprint!("Signing secret: ");
                            print!("{}\n", webhook.signing_secret);
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
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
