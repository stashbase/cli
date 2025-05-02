use log::debug;

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{api_client::RequestApiOptionResponse, webhooks::RotateWebhookSecretResponse},
    utils::{interaction, spinner::request_spinner},
};

pub type RotateWebhookSecretArgs = webhooks::RotateArgs;

pub async fn handle_rotate_webhook_secret(args: RotateWebhookSecretArgs) -> Result<()> {
    let i = interaction::confirm_opt("Are you sure you want to rotate signing secret?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let mut spinner = request_spinner();

    let res = webhooks::rotate_secret(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    // safe
    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(res_data) => {
            if let Some(res_text) = res_data.text {
                let data = serde_json::from_str::<RotateWebhookSecretResponse>(&res_text);

                match data {
                    Ok(data) => {
                        spinner.stop_with_message("Webhook secret rotated.");
                        // spinner.stop_with_message("✅ Webhook secret has been rotated!");
                        println!("\nSigning secret: {}", &data.signing_secret);
                    }
                    Err(e) => {
                        spinner.stop_and_persist("", "");
                        debug!("Err: {}", e);
                        bail!("Something went wrong")
                    }
                }
            } else {
                panic!();
            }
        }
        RequestApiOptionResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
