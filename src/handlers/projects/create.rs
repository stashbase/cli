use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects, models::projects::CreateProjectPayload, utils::spinner::request_spinner,
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
    spinner.stop_and_persist("", "");

    if let Err(err) = &project_res {
        error!("{:#?}", &err);
        bail!("Could not connect to API")
    }

    let project_res = project_res.unwrap();
    let status = project_res.status();

    if status == 401 {
        bail!("Unauthorized")
    } else if !status.is_success() {
        bail!("Something went wrong")
    } else if status.is_success() {
        println!("Project created");
    }

    Ok(())
}
