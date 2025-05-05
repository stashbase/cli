use anyhow::{bail, Result};
use log::error;
use serde::Deserialize;

use crate::{
    api::projects,
    models::api_client::{GetRequestApiResponse, OutputError},
    utils::spinner::request_spinner,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenProjectResponse {
    dashboard_url: String,
}

pub async fn handle_open_project(api_key: String, name: String, json_format: bool) -> Result<()> {
    // send request
    let mut spinner = request_spinner();
    let project_res = projects::get_project_dashboard_url(api_key, name).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");

        let formatted_err = err.format_error_output(json_format)?;
        bail!(formatted_err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<OpenProjectResponse>(&data.text);

            match data {
                Ok(data) => {
                    let url = data.dashboard_url;
                    spinner.stop_with_message(&format!("Opening URL: {}", url));

                    if let Err(err) = webbrowser::open(&url) {
                        spinner.stop_with_message(&format!("Error opening URL: {}", err));
                    }
                }
                Err(e) => {
                    spinner.stop_and_persist("", "");
                    error!("{}", e);

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
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
