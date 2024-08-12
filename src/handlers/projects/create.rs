use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{
        api_client::RequestApiOptionResponse,
        projects::CreateProjectPayload,
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::{
        spinner::request_spinner,
        validation::{resource_name_has_id_format, validate_project_name, IdentifierResource},
    },
};

pub async fn handle_create_project(
    api_key: String,
    name: String,
    description: Option<String>,
) -> Result<()> {
    let name_is_valid = validate_project_name(&name, false, true);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    let name_has_id_format = resource_name_has_id_format(IdentifierResource::Project, &name);

    if name_has_id_format {
        let error = InputValidationError::Projects(ProjectInputValidationError::NameUsingIdFormat);
        bail!(error)
    }

    debug!("creating project...:");

    let data = CreateProjectPayload { name, description };

    let mut spinner = request_spinner();

    let project_res = projects::create_project(api_key, &data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(format!("Error sending request: {}", err));
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(_) => {
            spinner.stop_with_message("🔥 Project created!");
        }
        RequestApiOptionResponse::Err(e) => {
            // spinner.stop_and_persist("", "");
            // eprint!("{}", e);
            // error!("{:#?}", &e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
