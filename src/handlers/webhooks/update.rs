use anyhow::bail;
use log::debug;

use crate::{
    api::webhooks,
    models::{
        api_client::RequestApiOptionResponse,
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
    pub json_format: bool,
    // data
    pub url: Option<String>,
    pub description: Option<String>,
}

pub async fn handle_update_webhook(args: UpdateWebhookArgs) -> anyhow::Result<()> {
    debug!("{:#?}", &args);

    let UpdateWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
        url,
        description,
        json_format,
    } = args;

    let validation_res = validate_input(&url, &description);

    if let Err(e) = validation_res {
        let formatted_err = e.format_error_output(false)?;

        eprintln!();
        bail!(formatted_err);
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

        let error_str = err.format_error_output(json_format)?;
        bail!(error_str);
    }

    // safe
    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            // println!("Project has been deleted");
            if json_format {
                spinner.stop_and_persist("", "");
                println!("{{}}");
            } else {
                spinner.stop_with_message("Webhook updated.");
            }
        }
        RequestApiOptionResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_and_persist("", "");

            let error_str = e.format_error_output(json_format)?;
            bail!(error_str);
        }
    }

    Ok(())
}

pub fn validate_input(
    new_name: &Option<String>,
    new_description: &Option<String>,
) -> Result<(), InputValidationError> {
    if new_name.is_none() && new_description.is_none() {
        let err = InputValidationError::Webhook(WebhookInputValidationError::NoUpdateFlags);
        return Err(err);
    }

    Ok(())
}
