use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::webhooks,
    models::{
        api_client::PostPatchRequestApiResponse,
        validation::{InputValidationError, WebhookInputValidationError},
        webhooks::UpdateWebhookPayload,
    },
    utils::spinner::request_spinner,
};

#[derive(Debug)]
pub struct UpdateWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    // data
    pub url: Option<String>,
    pub description: Option<String>,
}

pub async fn handle_update_webhook(args: UpdateWebhookArgs) -> Result<()> {
    debug!("{:#?}", &args);

    let UpdateWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
        url,
        description,
    } = args;

    let validation_res = validate_input(&"", &url, &description);

    if let Err(e) = validation_res {
        bail!("{}", e);
    }

    let args = webhooks::UpdateArgs {
        api_key,
        project,
        environment,
        webhook_id,
        data: UpdateWebhookPayload { url, description },
    };

    let mut spinner = request_spinner();

    let res = webhooks::update(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    // safe
    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(_) => {
            // println!("Project has been deleted");
            spinner.stop_with_message("✏️ Webhook has been updated!");
        }
        PostPatchRequestApiResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}

pub fn validate_input(
    name: &str,
    new_name: &Option<String>,
    new_description: &Option<String>,
) -> Result<()> {
    if new_name.is_none() && new_description.is_none() {
        let err = InputValidationError::Webhook(WebhookInputValidationError::NoUpdateFlags);
        bail!(err)
    }

    Ok(())
}
