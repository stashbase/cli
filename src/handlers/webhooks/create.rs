use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        webhooks::{CreateWebhookPayload, Webhook},
    },
    utils::{output::get_formatted_json_string, spinner::request_spinner},
};

pub struct CreateWebhookArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
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
        enable,
        json_format,
        silent,
    } = args;

    let args = webhooks::CreateArgs {
        api_key,
        project,
        environment,
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
        RequestApiOptionResponse::Ok(data) => match data.text {
            Some(text_data) => {
                let webhook = serde_json::from_str::<Webhook>(&text_data);

                match webhook {
                    Ok(webhook) => {
                        if json_format {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }
                            let json_str = get_formatted_json_string(&webhook, true).unwrap();
                            println!("{}", json_str);

                            return Ok(());
                        }

                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        print!("{}", webhook);
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
        },
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
