use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::environments,
    models::{
        api_client::RequestApiOptionResponse,
        environments::UpdateEnvironmentPayload,
        validation::{EnvironmentsInputValidationError, InputValidationError},
    },
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{
            resource_name_has_id_format, validate_environment_name, validate_project_name,
            IdentifierResource,
        },
    },
};

pub async fn handle_update_environment(
    api_key: String,
    project: String,
    environment: String,
    new_name: Option<String>,
    new_description: Option<String>,
    new_is_production: Option<bool>,
) -> Result<()> {
    // validation
    let input_valid_res = validate_input(
        &project,
        &environment,
        &new_name,
        &new_description,
        &new_is_production,
    );

    if let Err(err) = input_valid_res {
        eprintln!();
        bail!(err);
    }

    // OK
    debug!("updating project...:");

    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    let data = UpdateEnvironmentPayload {
        name: new_name,
        description: new_description,
        is_production: new_is_production,
    };

    let mut spinner = request_spinner();
    let project_res = environments::update(api_key, project, environment, &data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(_) => {
            spinner.stop_with_message("Environment updated.");
        }
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    Ok(())
}

pub fn validate_input(
    project: &str,
    environment: &str,
    new_env_name: &Option<String>,
    new_description: &Option<String>,
    new_is_production: &Option<bool>,
) -> Result<()> {
    let project_name_is_valid = validate_project_name(&project, false, false);

    if let Err(err) = project_name_is_valid {
        bail!(err);
    }

    let env_name_validation_res = validate_environment_name(environment, false, true);

    if let Err(err) = env_name_validation_res {
        bail!(err);
    }

    if new_env_name.is_none() && new_description.is_none() && new_is_production.is_none() {
        let err =
            InputValidationError::Environments(EnvironmentsInputValidationError::NoUpdateFlags);
        bail!(err)
    }

    if let Some(new_name) = &new_env_name {
        let new_name_is_id = resource_name_has_id_format(IdentifierResource::Environment, new_name);

        if new_name_is_id {
            let err = InputValidationError::Environments(
                EnvironmentsInputValidationError::NameUsingIdFormat,
            );
            bail!(err)
        }

        let name_is_id = resource_name_has_id_format(IdentifierResource::Environment, environment);

        if *new_name == environment && !name_is_id {
            let err = InputValidationError::Environments(
                EnvironmentsInputValidationError::NewNameEqualsOriginal,
            );
            bail!(err)
        }

        // TODO new arg
        let new_name_is_valid = validate_environment_name(new_name, true, true);

        if let Err(err) = new_name_is_valid {
            bail!(err);
        }
    }

    Ok(())
}
