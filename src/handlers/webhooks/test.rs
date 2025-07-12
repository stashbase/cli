use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::webhooks,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        webhooks::TestWebhookResponse,
    },
    utils::{interaction, output::get_colored_json, spinner::request_spinner},
};

#[derive(Debug)]
pub struct TestWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_test_webhook(args: TestWebhookArgs) -> Result<()> {
    debug!("{:#?}", &args);

    let TestWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
        json_format,
        silent,
    } = args;

    if !silent {
        let msg = "Test webhook event will be sent the webhook URL.";
        eprintln!("{}", msg.yellow());

        // eprintln!();
        let i = interaction::confirm_opt("Are you sure?");

        if i.is_none() || (i.unwrap() == false) {
            return Ok(());
        }
    }

    let args = webhooks::TestArgs {
        api_key,
        project,
        environment,
        webhook_id,
    };

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = webhooks::test(args).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_str = err.format_error_output(json_format)?;
        bail!(error_str);
    }

    // safe
    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(data) => {
            debug!("{:#?}", &data);

            if let Some(res_text) = data.text {
                debug!("{:#?}", &res_text);
                let test_response_json = serde_json::from_str::<TestWebhookResponse>(&res_text);

                match test_response_json {
                    Ok(test_res) => {
                        if json_format {
                            let json_str = get_colored_json(&test_res).unwrap();

                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }
                            println!("{}", json_str);
                        } else {
                            if let Some(mut spinner) = spinner {
                                spinner.stop_and_persist("", "");
                            }
                            print!("{}", &test_res);
                        }
                    }
                    Err(e) => {
                        debug!("{}", e);
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        bail!(formatted_err);
                    }
                }
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let error = OutputError::failed_to_deserialize_response_body();
                let formatted_err = error.format_error_output(json_format)?;

                bail!(formatted_err);
            }
            //
        }
        RequestApiOptionResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_str = e.format_error_output(json_format)?;
            bail!(error_str);
        }
    }

    Ok(())
}
