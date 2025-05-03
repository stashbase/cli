use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::webhooks,
    models::{
        api_client::RequestApiOptionResponse,
        webhooks::{CreateWebhookPayload, CreateWebhookResponse},
    },
    utils::{output::get_colored_json, spinner::request_spinner},
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
    pub json_format: bool,
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
        json_format,
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

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
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
                            if json_format {
                                let json_str = get_colored_json(&webhook).unwrap();

                                spinner.stop_and_persist("", "");
                                println!("{}", json_str);

                                return Ok(());
                            }

                            let msg = match enable {
                                true => "Webhook created and enabled.",
                                false => "Webhook created.",
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
                            bail!("Something went wrong.")
                        }
                    }
                }
                None => {
                    // NOTE: webhook creted but no id returned
                    spinner.stop_and_persist("", "");
                    bail!("Something went wrong.")
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
