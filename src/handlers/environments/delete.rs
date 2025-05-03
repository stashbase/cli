use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    models::api_client::DeleteRequestApiResponse,
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{validate_project_environment, validate_project_environment_identifier},
    },
};

pub async fn handle_delete_environment(
    api_key: String,
    project: String,
    environment: String,
    json_format: bool,
) -> Result<()> {
    let input_valid = validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_valid {
        eprintln!();
        bail!(err);
    }
    // ok

    eprintln!("{}", "Environment with all secrets will be deleted.".red());

    let i = interaction::input(&format!("Type '{}' to confirm.", environment));

    if i != environment {
        println!("Input does not match, action aborted.");
        return Ok(());
    }

    debug!("deleting enironment...:");

    let mut spinner = request_spinner();
    let res = environments::delete(api_key, project, environment).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        DeleteRequestApiResponse::Ok(_) => {
            if json_format {
                spinner.stop_and_persist("", "");
                println!("{{}}");
            } else {
                spinner.stop_with_message("Environment deleted.");
            }
        }
        DeleteRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    Ok(())
}
