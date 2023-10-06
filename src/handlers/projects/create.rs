use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{api_client::PostRequestApiResponse, projects::CreateProjectPayload},
    utils::spinner::request_spinner,
};

pub async fn handle_create_project(
    token: String,
    name: String,
    description: Option<String>,
) -> Result<()> {
    debug!("creating project...:");

    let data = CreateProjectPayload { name, description };

    let mut spinner = request_spinner();

    let project_res = projects::create_project(token, data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        PostRequestApiResponse::Ok(_) => {
            spinner.stop_with_message("🔥 Project created!");
        }
        PostRequestApiResponse::Err(e) => {
            // spinner.stop_and_persist("", "");
            // eprint!("{}", e);
            // error!("{:#?}", &e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
