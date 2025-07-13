use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::webhooks,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
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
    pub silent: bool,
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
        silent,
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

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = webhooks::create(args).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

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

                                if let Some(mut spinner) = spinner {
                                    spinner.stop_and_persist("", "");
                                }
                                println!("{}", json_str);

                                return Ok(());
                            }

                            if !silent {
                                if let Some(mut spinner) = spinner {
                                    let msg = match enable {
                                        true => "Webhook created and enabled.",
                                        false => "Webhook created.",
                                    };

                                    spinner.stop_with_message(msg);
                                }
                                eprint!("Id: ");
                                print!("{}\n", webhook.id);

                                eprint!("Signing secret: ");
                                print!("{}\n", webhook.signing_secret);
                            } else {
                                eprintln!("Id: {}", webhook.id);
                                eprintln!("Signing secret: {}", webhook.signing_secret);
                            }
                        }
                        Err(_) => {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }

                            let error = OutputError::failed_to_deserialize_response_body();
                            let formatted_err = error.format_error_output(json_format)?;

                            bail!(formatted_err);
                        }
                    }
                }
                None => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    bail!(formatted_err);
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
