use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{
        api_client::PostPatchRequestApiResponse,
        projects::UpdateProjectPayload,
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::{interaction, spinner::request_spinner, validation::validate_project_name},
};

pub async fn handle_update_project(
    token: String,
    name: String,
    new_name: Option<String>,
    new_description: Option<String>,
) -> Result<()> {
    if new_name.is_none() && new_description.is_none() {
        let err = InputValidationError::Projects(ProjectInputValidationError::NoUpdateFlags);
        bail!(err)
    }

    let name_is_valid = validate_project_name(&name, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    if let Some(new_name) = &new_name {
        if *new_name == name {
            let err = InputValidationError::Projects(ProjectInputValidationError::SameNewName);
            bail!(err)
        }

        let new_name_is_valid = validate_project_name(new_name, true);

        if let Err(err) = new_name_is_valid {
            bail!(err);
        }
    }

    debug!("updating project...:");

    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let data = UpdateProjectPayload {
        name: new_name,
        description: new_description,
    };

    let mut spinner = request_spinner();
    let project_res = projects::update_project(token, name, data).await;
    spinner.stop_and_persist("", "");

    if let Err(err) = project_res {
        eprintln!();
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        PostPatchRequestApiResponse::Ok(_) => {
            // println!("Project has been deleted");
            println!("✏️ Project has been updated!");
        }
        PostPatchRequestApiResponse::Err(e) => {
            // error!("{:#?}", &e);
            // eprint!("{}", e);
            eprintln!("{}", e);
        }
    }

    Ok(())
}
