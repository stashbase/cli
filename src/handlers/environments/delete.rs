use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    models::api_client::DeleteRequestApiResponse,
    utils::{interaction, spinner::request_spinner, validation::validate_project_name},
};

pub async fn handle_delete_environment(
    token: String,
    project: String,
    environment: String,
) -> Result<()> {
    let name_is_valid = validate_project_name(&project, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    eprintln!("{}", "Environment with all secrets will be deleted".red());

    let i = interaction::input(&format!("Type '{}' to confirm", environment));

    if i != environment {
        println!("Input does not match, action aborted");
        return Ok(());
    }

    debug!("deleting enironment...:");

    let mut spinner = request_spinner();
    let res = environments::delete(token, project, environment).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        DeleteRequestApiResponse::Ok => {
            spinner.stop_with_message("🗑️ Environment has been deleted!");
        }
        DeleteRequestApiResponse::Err(e) => {
            // error!("{:#?}", &e);
            // eprint!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
