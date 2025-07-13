use anyhow::bail;
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

pub struct HandleUpdateProjectArgs {
    pub api_key: String,
    pub name: String,
    pub new_name: Option<String>,
    pub new_description: Option<String>,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

pub async fn handle_update_project(args: HandleUpdateProjectArgs) -> anyhow::Result<()> {
    let HandleUpdateProjectArgs {
        api_key,
        name,
        new_name,
        new_description,
        json_format,
        silent,
        force,
    } = args;

    let validation_res = validate_input(&name, &new_name, &new_description);

    if let Err(e) = validation_res {
        let error_output = e.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    debug!("updating project...:");

    if !force {
        let i = interaction::confirm_opt("Are you sure?");

        if i.is_none() || (i.unwrap() == false) {
            return Ok(());
        }
    }

    let data = UpdateProjectPayload {
        name: new_name,
        description: new_description,
    };

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = projects::update_project(api_key, name, &data).await;

    if let Err(err) = project_res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        error!("{:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                println!("{{}}");
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Project updated.");
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}

pub fn validate_input(
    name: &str,
    new_name: &Option<String>,
    new_description: &Option<String>,
) -> Result<(), InputValidationError> {
    if new_name.is_none() && new_description.is_none() {
        let err = InputValidationError::Projects(ProjectInputValidationError::NoUpdateFlags);
        return Err(err);
    }

    let identifier_validation_res = validate_project_identifier(&name, true);

    if let Err(err) = identifier_validation_res {
        return Err(err);
    }

    if let Some(new_name) = &new_name {
        let new_name_is_id = resource_name_has_id_format(IdentifierResource::Project, new_name);

        if new_name_is_id {
            let err =
                InputValidationError::Projects(ProjectInputValidationError::NameUsingIdFormat);
            return Err(err);
        }

        let name_is_id = resource_name_has_id_format(IdentifierResource::Project, name);

        if *new_name == name && !name_is_id {
            let err =
                InputValidationError::Projects(ProjectInputValidationError::NewNameEqualsOriginal);
            return Err(err);
        }

        let new_name_is_valid = validate_project_name(new_name, true, true);

        if let Err(err) = new_name_is_valid {
            return Err(err);
        }
    }

    Ok(())
}
