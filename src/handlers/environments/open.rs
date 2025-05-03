use anyhow::{bail, Result};
use log::error;
use serde::Deserialize;

use crate::{
    api::environments,
    models::api_client::GetRequestApiResponse,
    utils::{spinner::request_spinner, validation::validate_project_environment_identifier},
};

#[derive(Debug, Deserialize)]
pub struct GetEnvUrlResponse {
    #[serde(rename = "dashboardUrl")]
    pub dashboard_url: String,
}

pub async fn handle_open_environment(
    api_key: String,
    project: String,
    environment: String,
) -> Result<()> {
    let input_validation_res =
        validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_validation_res {
        eprintln!();
        bail!(err);
    }

    // OK
    // send request
    let mut spinner = request_spinner();
    let project_res = environments::get_url(api_key, project, environment).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<GetEnvUrlResponse>(&data.text);

            match data {
                Ok(data) => {
                    let url = data.dashboard_url;
                    spinner.stop_with_message(&format!("Opening URL: {}", url));

                    if let Err(err) = webbrowser::open(&url) {
                        spinner.stop_with_message(&format!("Error opening URL: {}", err));
                    }
                }
                Err(e) => {
                    error!("{:#?}", e);
                    spinner.stop_and_persist("", "");
                    bail!("Something went wrong.");
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    // if let Err(err) = webbrowser::open(&url) {
    //     eprintln!("Error opening URL: {}", err);
    // }
    //
    Ok(())
}
