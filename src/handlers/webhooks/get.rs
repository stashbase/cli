use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::webhooks,
    models::{api_client::GetRequestApiResponse, webhooks::Webhook},
    utils::spinner::request_spinner,
};

#[derive(Debug)]
pub struct GetWebhookArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub with_secret: bool,
    pub format_json: bool,
}

impl From<GetWebhookArgs> for webhooks::GetArgs {
    fn from(args: GetWebhookArgs) -> webhooks::GetArgs {
        webhooks::GetArgs {
            api_key: args.api_key,
            project: args.project,
            environment: args.environment,
            webhook_id: args.webhook_id,
            with_secret: args.with_secret,
        }
    }
}

pub async fn handle_get_webhook(args: GetWebhookArgs) -> Result<()> {
    debug!("{:#?}", &args);
    debug!("listing env webhooks...");

    let format_json = args.format_json;
    let args: webhooks::GetArgs = args.into();

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
