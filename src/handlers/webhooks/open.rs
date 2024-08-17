use anyhow::{bail, Result};
use log::error;
use serde::Deserialize;

use crate::{
    api::{environments, webhooks},
    models::api_client::GetRequestApiResponse,
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
) -> Result<()> {
    // send request
    let mut spinner = request_spinner();

    let request_res = match &webhook_id {
        Some(id) => webhooks::get_dashboard_url(api_key, project, environment, id).await,
        None => environments::get_url(api_key, project, environment).await,
    };

    if let Err(err) = request_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
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

                    spinner.stop_with_message(&format!("Opening URL: {}", url));

                    if let Err(err) = webbrowser::open(&url) {
                        spinner.stop_with_message(&format!("Error opening URL: {}", err));
                    }
                }
                Err(e) => {
                    error!("{:#?}", e);
                    bail!("Something went wrong");
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    // if let Err(err) = webbrowser::open(&url) {
    //     eprintln!("Error opening URL: {}", err);
    // }
    //
    Ok(())
}
