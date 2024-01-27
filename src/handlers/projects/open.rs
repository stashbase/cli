use anyhow::{bail, Result};
use log::error;
use serde::Deserialize;

use crate::{
    api::projects, models::api_client::GetRequestApiResponse, utils::spinner::request_spinner,
};

#[derive(Debug, Deserialize)]
struct OpenProjectResponse {
    url: String,
}

pub async fn handle_open_project(api_key: String, name: String) -> Result<()> {
    // send request
    let mut spinner = request_spinner();
    let project_res = projects::get_project_url(api_key, name).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            let data = serde_json::from_str::<OpenProjectResponse>(&data.text);

            match data {
                Ok(data) => {
                    let url = data.url;
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
