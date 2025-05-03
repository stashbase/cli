use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{
        api_client::RequestApiOptionResponse,
        projects::{CreateProjectPayload, CreateProjectResponse},
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
        eprintln!("");
        bail!(err);
    }

    let name_has_id_format = resource_name_has_id_format(IdentifierResource::Project, &name);

    if name_has_id_format {
        let error = InputValidationError::Projects(ProjectInputValidationError::NameUsingIdFormat);

        eprintln!("");
        bail!(error)
    }

    debug!("creating project...:");

    let data = CreateProjectPayload { name, description };

    let mut spinner = request_spinner();

    let project_res = projects::create_project(api_key, &data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let response = serde_json::from_str::<CreateProjectResponse>(&text);

                match response {
                    Ok(data) => {
                        // let msg = format!("🔥 Project with id {} created!", data.id);
                        // spinner.stop_with_message(&msg);

                        let msg = format!("Project created.");
                        spinner.stop_with_message(&msg);

                        eprint!("Id: ");
                        print!("{}\n", data.id);
                    }
                    Err(e) => {
                        spinner.stop_and_persist("", "");
                        error!("{:#?}", e);
                        bail!("{}", "Something went wrong.");
                    }
                }
            }
            None => {
                spinner.stop_and_persist("", "");
                bail!("{}", "Something went wrong.");
            }
        },
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
