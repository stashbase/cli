use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::webhooks,
    cmd::environments::EnvironmentFormat,
    models::{
        api_client::GetRequestApiResponse,
        validation::{InputValidationError, WebhookInputValidationError},
        webhooks::{TableWebhookLog, WebhookLogList},
    },
    utils::{
        spinner::request_spinner,
        tables::{self},
    },
};

#[derive(Debug)]
pub struct ListWebhookLogsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub page: Option<usize>,
    pub format: EnvironmentFormat,
    pub per_page: Option<u8>,
}

pub async fn handle_list_webhook_logs(args: ListWebhookLogsArgs) -> Result<()> {
    let ListWebhookLogsArgs {
        api_key,
        project,
        environment,
        webhook_id,
        page,
        format,
        per_page,
    } = args;

    if let Some(per_page) = per_page {
        let is_valid = per_page == 5 || per_page == 10 || per_page == 15 || per_page == 20;

        if !is_valid {
            let webhook_error = WebhookInputValidationError::InvalidPerPage;
            let err = InputValidationError::Webhook(webhook_error);

            bail!(err);
        }
    }

    let args = webhooks::ListLogsArgs {
        api_key,
        project,
        environment,
        webhook_id,
        page,
        per_page,
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
            spinner.stop_and_persist("", "");

            debug!("{:#?}", &data.text);
            let data = serde_json::from_str::<WebhookLogList>(&data.text);

            match data {
                Ok(webhook_logs) => match format {
                    EnvironmentFormat::List => {
                        print!("{}", webhook_logs);
                    }
                    EnvironmentFormat::Json => {
                        let value = serde_json::to_value(&webhook_logs).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();
                        println!("{}", pretty);
                    }
                    EnvironmentFormat::Table => {
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

                        println!("{} {}/{}", "Pages:", page.unwrap_or(1), webhook_logs.pages);

                    }
                },
                Err(e) => {
                    debug!("Error: {:#?}", &e);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
