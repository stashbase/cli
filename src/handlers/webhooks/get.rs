use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;
use short_uuid::ShortUuid;

use crate::{
    api::webhooks,
    models::{
        api_client::GetRequestApiResponse,
        validation::{InputValidationError, WebhookInputValidationError},
        webhooks::Webhook,
    },
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

#[derive(Debug)]
pub struct GetWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub format_json: bool,
}

pub async fn handle_get_webhook(args: GetWebhookArgs) -> Result<()> {
    debug!("{:#?}", &args);

    let GetWebhookArgs {
        api_key,
        project,
        environment,
        webhook_id,
        format_json,
    } = args;

    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    debug!("listing env webhooks...");

    let parsed = ShortUuid::parse_str(&webhook_id);

    if let Err(_) = parsed {
        let input_err = WebhookInputValidationError::InvalidId;

        bail!(InputValidationError::Webhook(input_err));
    }

    let args = webhooks::GetArgs {
        api_key,
        project,
        environment,
        webhook_id,
    };

    let mut spinner = request_spinner();

    let res = webhooks::get(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    // safe
    let res = res.unwrap();

    // TODO: sort
    match res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            let webhook = serde_json::from_str::<Webhook>(&data.text);

            match webhook {
                Ok(webhook) => {
                    spinner.stop_and_persist("", "");

                    if format_json == true {
                        spinner.stop_and_persist("", "");
                        let value = serde_json::to_value(&webhook).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    } else {
                        print!("{}", webhook);
                    }
                }
                Err(e) => {
                    spinner.stop_and_persist("", "");
                    debug!("Err: {}", e);
                    bail!("Something went wrong")
                }
            }

            // match webhooks {
            //     Ok(webhooks) => match format {
            //         EnvironmentFormat::List => {
            //             for (i, p) in webhooks.iter().enumerate() {
            //                 if i == webhooks.len() - 1 {
            //                     print!("{}", p);
            //                 } else {
            //                     println!("{}", p);
            //                 }
            //             }
            //         }
            //         EnvironmentFormat::Json => {
            //             spinner.stop_and_persist("", "");
            //             let value = serde_json::to_value(&webhooks).unwrap();
            //             let pretty = to_colored_json_auto(&value).unwrap();
            //
            //             println!("{}", pretty);
            //         }
            //         EnvironmentFormat::Table => {
            //             let reversed = webhooks.into_iter().rev().collect();
            //             let table = tables::build::build_table(&reversed);
            //             println!("{}", table);
            //         }
            //     },
            //     Err(e) => {
            //         spinner.stop_and_persist("", "");
            //         debug!("Err: {}", e);
            //         bail!("Something went wrong")
            //     }
            // }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
