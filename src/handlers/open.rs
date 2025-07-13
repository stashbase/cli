use anyhow::{bail, Result};
use serde::Deserialize;

use crate::{
    api::workspace, models::api_client::GetRequestApiResponse, utils::spinner::request_spinner,
};

#[derive(Debug, Deserialize)]
struct OpenDashboardResponse {
    #[serde(rename = "dashboardUrl")]
    dashboard_url: String,
}

pub async fn handle_open_dashboard(api_key: String, silent: bool) -> Result<()> {
    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let response = workspace::get_url(api_key).await;

    if let Err(err) = response {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        bail!(err);
    }

    let project_res = response.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<OpenDashboardResponse>(&data.text);

            match data {
                Ok(data) => {
                    let url = data.dashboard_url;

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

                    bail!("Something went wrong.");
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            bail!(e);
        }
    }

    // if let Err(err) = webbrowser::open(&url) {
    //     eprintln!("{}", &format!("Error opening URL: {}", err));
    // }
    Ok(())
}
