use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    handlers::environments::update::validate_input,
    models::{api_client::PostPatchRequestApiResponse, environments::DuplicateEnvironmentPayload},
    utils::{interaction, spinner::request_spinner},
};

pub async fn handle_duplicate_environment(
    token: String,
    project: String,
    environment: String,
    new_name: String,
) -> Result<()> {
    // validation
    let input_valid_res = validate_input(&project, &environment, &Some(new_name.clone()), &None);

    if let Err(err) = input_valid_res {
        bail!(err);
    }

    if environment == new_name {
        let err = format!(
            "{}\n- message: {}",
            "Input error".red().bold(),
            "Duplicate name is the same",
        );
        bail!(err);
    }

    // OK
    debug!("updating project...:");

    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let data = DuplicateEnvironmentPayload { name: new_name };

    let mut spinner = request_spinner();
    let project_res = environments::duplicate(token, project, environment, &data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        PostPatchRequestApiResponse::Ok(_) => {
            spinner.stop_with_message("Environment has been cloned!");
        }
        PostPatchRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
