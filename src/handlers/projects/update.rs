use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{
        api_client::RequestApiOptionResponse,
        projects::UpdateProjectPayload,
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{
            resource_name_has_id_format, validate_project_identifier, validate_project_name,
            IdentifierResource,
        },
    },
};

pub async fn handle_update_project(
    api_key: String,
    name: String,
    new_name: Option<String>,
    new_description: Option<String>,
) -> Result<()> {
    let validation_res = validate_input(&name, &new_name, &new_description);

    if let Err(e) = validation_res {
        bail!("{}", e);
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
    let project_res = projects::update_project(api_key, name, &data).await;

    if let Err(err) = project_res {
        // eprintln!();
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(_) => {
            // println!("Project has been deleted");
            spinner.stop_with_message("Project has been updated.");
        }
        RequestApiOptionResponse::Err(e) => {
            // eprintln!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}

pub fn validate_input(
    name: &str,
    new_name: &Option<String>,
    new_description: &Option<String>,
) -> Result<()> {
    if new_name.is_none() && new_description.is_none() {
        let err = InputValidationError::Projects(ProjectInputValidationError::NoUpdateFlags);
        bail!(err)
    }

    let identifier_validation_res = validate_project_identifier(&name, true);

    if let Err(err) = identifier_validation_res {
        bail!(err);
    }

    if let Some(new_name) = &new_name {
        let new_name_is_id = resource_name_has_id_format(IdentifierResource::Project, new_name);

        if new_name_is_id {
            let err =
                InputValidationError::Projects(ProjectInputValidationError::NameUsingIdFormat);
            bail!(err)
        }

        let name_is_id = resource_name_has_id_format(IdentifierResource::Project, name);

        if *new_name == name && !name_is_id {
            let err =
                InputValidationError::Projects(ProjectInputValidationError::NewNameEqualsOriginal);
            bail!(err)
        }

        let new_name_is_valid = validate_project_name(new_name, true, true);

        if let Err(err) = new_name_is_valid {
            bail!(err);
        }
    }

    Ok(())
}
