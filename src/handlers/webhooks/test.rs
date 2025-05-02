use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::webhooks,
    models::{api_client::RequestApiOptionResponse, webhooks::TestWebhookResponse},
    utils::{interaction, spinner::request_spinner},
};

#[derive(Debug)]
pub struct TestWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
}

pub async fn handle_test_webhook(args: TestWebhookArgs) -> Result<()> {
    debug!("{:#?}", &args);

    let TestWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
    } = args;

    let msg = "Test webhook event will be sent the webhook URL.";
    eprintln!("{}", msg.yellow());

    // eprintln!();
    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let args = webhooks::TestArgs {
        api_key,
        project,
        environment,
        webhook_id,
    };

    let mut spinner = request_spinner();

    let res = webhooks::test(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
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
                        spinner.stop_and_persist("", "");
                        print!("{}", &test_res);
                    }
                    Err(e) => {
                        debug!("{}", e);
                        spinner.stop_and_persist("", "");
                        bail!("Something went wrong.");
                    }
                }
            } else {
                bail!("Something went wrong.");
            }
            //
        }
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
