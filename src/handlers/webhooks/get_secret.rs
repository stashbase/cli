use log::{debug, error};

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        webhooks::WebhookSigningSecret,
    },
    utils::{output::get_colored_json, spinner::request_spinner},
};

pub struct GetWebhookSecretArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub json_format: bool,
    pub silent: bool,
}

impl From<GetWebhookSecretArgs> for webhooks::GetSecretArgs {
    fn from(args: GetWebhookSecretArgs) -> webhooks::GetSecretArgs {
        webhooks::GetSecretArgs {
            api_key: args.api_key,
            project: args.project,
            environment: args.environment,
            webhook_id: args.webhook_id,
            json_format: args.json_format,
        }
    }
}

pub async fn handle_get_webhook_secret(args: GetWebhookSecretArgs) -> Result<()> {
    let json_format = args.json_format;
    let silent = args.silent;
    let args: webhooks::GetSecretArgs = args.into();

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = webhooks::get_secret(args).await;

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
        GetRequestApiResponse::Ok(data) => {
            let signing_secret_json = serde_json::from_str::<WebhookSigningSecret>(&data.text);

            match signing_secret_json {
                Ok(data) => {
                    if json_format {
                        let json_str = get_colored_json(&data).unwrap();

                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }
                        println!("{}", json_str);
                    } else {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_with_message("");
                        }
                        println!("{}", data.signing_secret);
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
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
