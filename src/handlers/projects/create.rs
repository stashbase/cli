use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        projects::{CreateProjectPayload, CreateProjectResponse},
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::{
        output::get_colored_json,
        spinner::request_spinner,
        validation::{resource_name_has_id_format, validate_project_name, IdentifierResource},
    },
};

pub async fn handle_create_project(
    api_key: String,
    name: String,
    description: Option<String>,
    json_format: bool,
) -> Result<()> {
    let name_is_valid = validate_project_name(&name, false, true);

    if let Err(err) = name_is_valid {
        let error_output = err.format_error_output(json_format)?;

        eprintln!();
        bail!(error_output);
    }

    let name_has_id_format = resource_name_has_id_format(IdentifierResource::Project, &name);

    if name_has_id_format {
        let error = InputValidationError::Projects(ProjectInputValidationError::NameUsingIdFormat);
        let error_output = error.format_error_output(json_format)?;

        eprintln!();
        bail!(error_output);
    }

    debug!("creating project...:");

    let data = CreateProjectPayload { name, description };

    let mut spinner = request_spinner();

    let project_res = projects::create_project(api_key, &data).await;

    if let Err(err) = project_res {
        error!("{:#?}", &err);
        spinner.stop_and_persist("", "");

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
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

                        if json_format {
                            let json_str = get_colored_json(&data).unwrap();

                            spinner.stop_and_persist("", "");
                            println!("{}", json_str);
                        } else {
                            let msg = format!("Project created.");
                            spinner.stop_with_message(&msg);

                            eprint!("Id: ");
                            print!("{}\n", data.id);
                        }
                    }
                    Err(e) => {
                        spinner.stop_and_persist("", "");
                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        bail!(formatted_err);
                    }
                }
            }
            None => {
                spinner.stop_and_persist("", "");
                bail!("Something went wrong.");
            }
        },
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
