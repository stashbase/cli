use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        projects::{CreateProjectPayload, Project},
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
    silent: bool,
) -> Result<()> {
    let name_is_valid = validate_project_name(&name, false, true);

    if let Err(err) = name_is_valid {
        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    let name_has_id_format = resource_name_has_id_format(IdentifierResource::Project, &name);

    if name_has_id_format {
        let error = InputValidationError::Projects(ProjectInputValidationError::NameUsingIdFormat);
        let error_output = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    if let Some(description) = &description {
        if description.len() > 255 {
            let error =
                InputValidationError::Projects(ProjectInputValidationError::DescriptionTooLong);

            let error_output = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }

            bail!(error_output);
        }
    }

    debug!("creating project...:");

    let data = CreateProjectPayload { name, description };

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = projects::create_project(api_key, &data).await;

    if let Err(err) = project_res {
        error!("{:#?}", &err);

        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(res) => {
            let text = match res.text {
                Some(text) => text,
                None => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    bail!("Something went wrong.");
                }
            };

            let data = match serde_json::from_str::<Project>(&text) {
                Ok(data) => data,
                Err(_) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let error = OutputError::failed_to_deserialize_response_body();
                    bail!(error.format_error_output(json_format)?);
                }
            };

            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            if json_format {
                println!("{}", get_colored_json(&data)?);
                return Ok(());
            }

            print!("{}", data);
        }

        RequestApiOptionResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            bail!(e.format_error_output(json_format)?);
        }
    }

    Ok(())
}
