use anyhow::{bail, Result};
use log::error;
use serde::Deserialize;

use crate::{
    api::{environments, webhooks},
    models::api_client::{GetRequestApiResponse, OutputError},
    utils::spinner::request_spinner,
};

#[derive(Debug, Deserialize)]
pub struct GetEnvWebhookUrlResponse {
    #[serde(rename = "dashboardUrl")]
    pub dashboard_url: String,
}

pub async fn handle_open_environment_webhook(
    api_key: String,
    project: String,
    environment: String,
    webhook_id: Option<String>,
    json_format: bool,
    silent: bool,
) -> Result<()> {
    // send request
    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let request_res = match &webhook_id {
        Some(id) => webhooks::get_dashboard_url(api_key, project, environment, id).await,
        None => environments::get_url(api_key, project, environment).await,
    };

    if let Err(err) = request_res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let formatted_err = err.format_error_output(json_format)?;
        bail!(formatted_err);
    }

    let project_res = request_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<GetEnvWebhookUrlResponse>(&data.text);

            match data {
                Ok(data) => {
                    let url = match webhook_id {
                        Some(_) => data.dashboard_url,
                        None => format!("{}/webhooks", data.dashboard_url),
                    };

                    if let Some(mut spinner) = spinner {
                        spinner.stop_with_message(&format!("Opening URL: {}", url));
                    }

                    if let Err(err) = webbrowser::open(&url) {
                        eprintln!("Error opening URL: {}", err);
                    }
                }
                Err(e) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                    error!("{}", e);

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let formatted_err = e.format_error_output(json_format)?;
            bail!(formatted_err);
        }
    }

    // if let Err(err) = webbrowser::open(&url) {
    //     eprintln!("Error opening URL: {}", err);
    // }
    //
    Ok(())
}
