use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::webhooks,
    cmd::configs::OutputFormat,
    models::{api_client::GetRequestApiResponse, webhooks::ListWebhook},
    utils::{spinner::request_spinner, tables},
};

#[derive(Debug)]
pub struct ListWebhooksArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    // TODO: rename type
    pub format: OutputFormat,
}

pub async fn handle_list_webhooks(args: ListWebhooksArgs) -> Result<()> {
    debug!("{:#?}", &args);

    let ListWebhooksArgs {
        api_key,
        project,
        environment,
        format,
    } = args;

    debug!("listing env webhooks...");

    let args = webhooks::ListArgs {
        api_key,
        project,
        environment,
    };

    let mut spinner = request_spinner();

    let res = webhooks::list(args).await;

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
            let webhooks = serde_json::from_str::<Vec<ListWebhook>>(&data.text);

            match webhooks {
                Ok(webhooks) => {
                    if webhooks.is_empty() {
                        spinner.stop_with_message("No webhooks found");
                    } else {
                        spinner.stop_and_persist("", "");

                        match format {
                            OutputFormat::List => {
                                for (i, p) in webhooks.iter().enumerate() {
                                    if i == webhooks.len() - 1 {
                                        print!("{}", p);
                                    } else {
                                        println!("{}", p);
                                    }
                                }
                            }
                            OutputFormat::Json => {
                                let value = serde_json::to_value(&webhooks).unwrap();
                                let pretty = to_colored_json_auto(&value).unwrap();

                                println!("{}", pretty);
                            }
                            OutputFormat::Table => {
                                let reversed = webhooks.into_iter().rev().collect();
                                let table = tables::build::build_table(&reversed);
                                println!("{}", table);
                            }
                        }
                    }
                }
                Err(e) => {
                    spinner.stop_and_persist("", "");
                    debug!("Err: {}", e);
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
