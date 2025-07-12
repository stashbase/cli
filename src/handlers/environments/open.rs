use anyhow::{bail, Result};
use log::error;
use serde::Deserialize;

use crate::{
    api::environments,
    models::api_client::{GetRequestApiResponse, OutputError},
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
    json_format: bool,
    silent: bool,
) -> Result<()> {
    let input_validation_res =
        validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_validation_res {
        let formatted_err = err.format_error_output(json_format)?;

        eprintln!();
        bail!(formatted_err);
    }

    // OK
    // send request
    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = environments::get_url(api_key, project, environment).await;

    if let Err(err) = project_res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let formatted_err = err.format_error_output(json_format)?;
        bail!(formatted_err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<GetEnvUrlResponse>(&data.text);

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
                    error!("{:#?}", e);

                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

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
