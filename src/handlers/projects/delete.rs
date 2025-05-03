use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::projects,
    models::api_client::DeleteRequestApiResponse,
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{validate_project_identifier, validate_project_name},
    },
};

pub async fn handle_delete_project(api_key: String, name: String) -> Result<()> {
    let identifier_is_valid = validate_project_identifier(&name, true);

    if let Err(err) = identifier_is_valid {
        eprintln!("");
        bail!(err);
    }

    eprintln!("{}", "All environments and secrets will be deleted.".red());

    let i = interaction::input(&format!("Type '{}' to confirm.", name));

    if i != name {
        eprintln!("Input does not match, action aborted.");
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
            spinner.stop_with_message("Project deleted.");
        }
        DeleteRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    Ok(())
}
