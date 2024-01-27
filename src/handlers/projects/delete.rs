use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::projects,
    models::api_client::DeleteRequestApiResponse,
    utils::{interaction, spinner::request_spinner, validation::validate_project_name},
};

pub async fn handle_delete_project(api_key: String, name: String) -> Result<()> {
    let name_is_valid = validate_project_name(&name, false, true);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    eprintln!("{}", "All environments and secrets will be deleted".red());

    let i = interaction::input(&format!("Type '{}' to confirm", name));

    if i != name {
        eprintln!("Input does not match, action aborted");
        return Ok(());
    }

    debug!("deleting project...:");

    let mut spinner = request_spinner();
    let project_res = projects::delete_project(api_key, name).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        DeleteRequestApiResponse::Ok(_) => {
            // println!("Project has been deleted");
            spinner.stop_with_message("🗑️ Project has been deleted!");
        }
        DeleteRequestApiResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
