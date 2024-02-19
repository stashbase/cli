use log::debug;

use anyhow::{bail, Result};

use crate::{
    api::webhooks,
    models::{api_client::PostPatchRequestApiResponse, webhooks::UpdateWebhookPayload},
    utils::{interaction, spinner::request_spinner, validation::validate_project_environment},
};

pub struct UpdateWebhookStatusArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub enabled: bool,
}

// TODO: return already enabled/disabled
pub async fn handle_update_webhook_status(args: UpdateWebhookStatusArgs) -> Result<()> {
    let UpdateWebhookStatusArgs {
        api_key,
        project,
        environment,
        webhook_id,
        enabled,
    } = args;

    let projec_env_valid = validate_project_environment(&project, &environment, true);

    if let Err(e) = projec_env_valid {
        bail!("{}", e);
    }

    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let args = webhooks::UpdateArgs {
        api_key,
        project,
        environment,
        webhook_id,
        data: UpdateWebhookPayload {
            url: None,
            description: None,
            enabled: Some(enabled),
        },
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

    // TODO: return already enabled/disabled
    match res {
        PostPatchRequestApiResponse::Ok(res_data) => {
            if let Some(res_text) = res_data.text {
            } else {
                let msg = match enabled {
                    true => "✅ Webhook has been enabled!",
                    // false => "❌ Webhook has been disabled!",
                    false => "✅ Webhook has been disabled!",
                };

                // println!("Project has been deleted");
                spinner.stop_with_message(msg);
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
