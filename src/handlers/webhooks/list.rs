use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::webhooks,
    cmd::config::OutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        webhooks::ListWebhook,
    },
    utils::{spinner::request_spinner, tables},
};

#[derive(Debug)]
pub struct ListWebhooksArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    // TODO: rename type
    pub format: OutputFormat,
    pub silent: bool,
}

pub async fn handle_list_webhooks(args: ListWebhooksArgs) -> Result<()> {
    debug!("{:#?}", &args);

    let ListWebhooksArgs {
        api_key,
        project,
        environment,
        format,
        silent,
    } = args;

    debug!("listing env webhooks...");

    let args = webhooks::ListArgs {
        api_key,
        project,
        environment,
    };

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = webhooks::list(args).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(format == OutputFormat::Json)?;
        bail!(error_output);
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
                        if let Some(mut spinner) = spinner {
                            if !silent {
                                spinner.stop_with_message("No webhooks found.");
                            } else {
                                spinner.stop_and_persist("", "");
                            }
                        } else if !silent {
                            println!("No webhooks found.");
                        }
                    } else {
                        if let Some(mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

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
                                let table = tables::build::build_table(&webhooks);
                                println!("{}", table);
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                    error!("{}", e);

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(format == OutputFormat::Json)?;

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(format == OutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}
