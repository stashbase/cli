use log::{debug, error};

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{api_client::GetRequestApiResponse, webhooks::WebhookSigningSecret},
    utils::spinner::request_spinner,
};

pub type GetWebhookSecretArgs = webhooks::GetSecretArgs;

pub async fn handle_get_webhook_secret(args: GetWebhookSecretArgs) -> Result<()> {
    let mut spinner = request_spinner();

    let res = webhooks::get_secret(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    // safe
    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let signing_secret_json = serde_json::from_str::<WebhookSigningSecret>(&data.text);

            match signing_secret_json {
                Ok(data) => {
                    // spinner.stop_and_persist("", &data.signing_secret);
                    // spinner.stop_with_message(&data.signing_secret);

                    spinner.stop_with_message("");
                    println!("{}", data.signing_secret);
                }
                Err(e) => {
                    error!("Err: {}", e);
                    spinner.stop_and_persist("", "");
                    bail!("{}", "Something went wrong.");
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
