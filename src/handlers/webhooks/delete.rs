use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::webhooks,
    models::api_client::DeleteRequestApiResponse,
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{validate_project_environment, validate_webhook_id},
    },
};

pub struct DeleteWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
}

pub async fn handle_delete_webhook(args: DeleteWebhookArgs) -> Result<()> {
    let DeleteWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
    } = args;

    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    let webhook_id_valdation = validate_webhook_id(&webhook_id);

    if let Err(e) = webhook_id_valdation {
        bail!(e)
    }

    // confirmation
    eprintln!("{}", "Do you really want to delete this webhook?".red());

    let i = interaction::input("Type 'DELETE' to confirm");

    if i != "DELETE" {
        println!("Input does not match, action aborted");
        return Ok(());
    }

    let args = webhooks::DeleteArgs {
        api_key,
        project,
        environment,
        webhook_id,
    };

    let mut spinner = request_spinner();

    let res = webhooks::delete(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        bail!(err);
    }

    let res = res.unwrap();

    debug!("{:#?}", &res);

    match res {
        DeleteRequestApiResponse::Ok(_) => {
            spinner.stop_with_message("🗑️ Webhook has been deleted!");
        }
        DeleteRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
