use log::{debug, error};

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        webhooks::RotateWebhookSecretResponse,
    },
    utils::{interaction, output::get_colored_json, spinner::request_spinner},
};

pub struct RotateWebhookSecretArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
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
            json_format: args.json_format,
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
        RequestApiOptionResponse::Ok(res_data) => {
            if let Some(res_text) = res_data.text {
                let data = serde_json::from_str::<RotateWebhookSecretResponse>(&res_text);

                match data {
                    Ok(data) => {
                        if json_format {
                            let json_str = get_colored_json(&data).unwrap();

                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }
                            println!("{}", json_str);
                        } else {
                            if !silent {
                                if let Some(mut spinner) = spinner {
                                    spinner.stop_with_message("Webhook secret rotated.");
                                }
                                println!("\nSigning secret: {}", &data.signing_secret);
                            } else {
                                if let Some(mut spinner) = spinner {
                                    spinner.stop_and_persist("", "");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }
                        error!("{}", e);

                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        bail!(formatted_err);
                    }
                }
            } else {
                panic!();
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
