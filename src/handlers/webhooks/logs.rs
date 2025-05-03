use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::webhooks,
    cmd::config::OutputFormat,
    models::{
        api_client::GetRequestApiResponse,
        validation::{InputValidationError, WebhookInputValidationError},
        webhooks::{TableWebhookLog, WebhookLogList},
    },
    utils::{spinner::request_spinner, tables},
};

#[derive(Debug)]
pub struct ListWebhookLogsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub page: Option<usize>,
    pub format: OutputFormat,
    pub limit: Option<usize>,
}

pub async fn handle_list_webhook_logs(args: ListWebhookLogsArgs) -> Result<()> {
    let ListWebhookLogsArgs {
        api_key,
        project,
        environment,
        webhook_id,
        page,
        format,
        limit,
    } = args;

    if let Some(limit) = limit {
        let is_valid = limit >= 2 && limit <= 30;

        if !is_valid {
            let webhook_error = WebhookInputValidationError::InvalidLimit;
            let err = InputValidationError::Webhook(webhook_error);

            eprintln!("");
            bail!(err);
        }
    }

    if let Some(page) = page {
        let is_valid = page > 0 && page <= 1000;

        if !is_valid {
            let webhook_error = WebhookInputValidationError::InvalidPage;
            let err = InputValidationError::Webhook(webhook_error);

            eprintln!("");
            bail!(err);
        }
    }

    let args = webhooks::ListLogsArgs {
        api_key,
        project,
        environment,
        webhook_id,
        page,
        limit,
    };

    let mut spinner = request_spinner();

    let res = webhooks::list_logs(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    // safe
    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            let data = serde_json::from_str::<WebhookLogList>(&data.text);

            match data {
                Ok(webhook_logs) => {
                    //

                    match format {
                        OutputFormat::List => {
                            spinner.stop_and_persist("", "");
                            // if webhook_logs.data == 0 {
                            //     eprintln!("No logs");
                            //     return Ok(());
                            // }

                            print!("{}", webhook_logs);
                        }
                        OutputFormat::Json => {
                            spinner.stop_and_persist("", "");
                            let value = serde_json::to_value(&webhook_logs).unwrap();
                            let pretty = to_colored_json_auto(&value).unwrap();
                            println!("{}", pretty);
                        }
                        OutputFormat::Table => {
                            if webhook_logs.data.is_empty() {
                                spinner.stop_with_message("No change.\n");
                                eprintln!("{}", webhook_logs.pagination);
                                // return Ok(());
                            } else {
                                spinner.stop_and_persist("", "");
                                let table_logs = webhook_logs
                                    .data
                                    .into_iter()
                                    .map(|item| {
                                        let table_item: TableWebhookLog = item.into();
                                        table_item
                                    })
                                    .collect();

                                let table = tables::build::build_table(&table_logs);
                                println!("{}", table);
                                eprintln!("\n{}", webhook_logs.pagination);
                            }
                        }
                    }
                }
                Err(e) => {
                    spinner.stop_and_persist("", "");
                    debug!("Error: {:#?}", &e);
                    bail!(e);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    Ok(())
}
