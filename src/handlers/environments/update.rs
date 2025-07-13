use anyhow::bail;
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

pub struct HandleUpdateEnvironmentArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub new_name: Option<String>,
    pub new_description: Option<String>,
    pub new_is_production: Option<bool>,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

pub async fn handle_update_environment(args: HandleUpdateEnvironmentArgs) -> anyhow::Result<()> {
    let HandleUpdateEnvironmentArgs {
        api_key,
        project,
        environment,
        new_name,
        new_description,
        new_is_production,
        json_format,
        silent,
        force,
    } = args;

    // validation
    let input_valid_res = validate_input(
        &project,
        &environment,
        &new_name,
        &new_description,
        &new_is_production,
    );

    if let Err(err) = input_valid_res {
        let error = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error);
    }

    // OK
    debug!("updating project...:");

    if !force {
        let i = interaction::confirm_opt("Are you sure?");

        if i.is_none() || (i.unwrap() == false) {
            return Ok(());
        }
    }

    let data = UpdateEnvironmentPayload {
        name: new_name,
        description: new_description,
        is_production: new_is_production,
    };

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = environments::update(api_key, project, environment, &data).await;

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
                    spinner.stop_with_message("Environment updated.");
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
    project: &str,
    environment: &str,
    new_env_name: &Option<String>,
    new_description: &Option<String>,
    new_is_production: &Option<bool>,
) -> Result<(), InputValidationError> {
    let project_name_is_valid = validate_project_name(&project, false, false);

    if let Err(err) = project_name_is_valid {
        return Err(err);
    }

    let env_name_validation_res = validate_environment_name(environment, false, true);

    if let Err(err) = env_name_validation_res {
        return Err(err);
    }

    if new_env_name.is_none() && new_description.is_none() && new_is_production.is_none() {
        let err =
            InputValidationError::Environments(EnvironmentsInputValidationError::NoUpdateFlags);
        return Err(err);
    }

    if let Some(new_name) = &new_env_name {
        let new_name_is_id = resource_name_has_id_format(IdentifierResource::Environment, new_name);

        if new_name_is_id {
            let err = InputValidationError::Environments(
                EnvironmentsInputValidationError::NameUsingIdFormat,
            );
            return Err(err);
        }

        let name_is_id = resource_name_has_id_format(IdentifierResource::Environment, environment);

        if *new_name == environment && !name_is_id {
            let err = InputValidationError::Environments(
                EnvironmentsInputValidationError::NewNameEqualsOriginal,
            );
            return Err(err);
        }

        // TODO new arg
        let new_name_is_valid = validate_environment_name(new_name, true, true);

        if let Err(err) = new_name_is_valid {
            return Err(err);
        }
    }

    Ok(())
}
