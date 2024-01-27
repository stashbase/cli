use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{api_client::PostPatchRequestApiResponse, secrets::UpdateSecretDescriptionPayload},
    utils::{
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_key},
    },
};

pub struct HandleDescriptionArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub key: String,
    pub description: String,
}

pub async fn handle_update_description(args: HandleDescriptionArgs) -> Result<()> {
    let HandleDescriptionArgs {
        api_key,
        project,
        environment,
        description,
        key,
    } = args;

    let input_validation_res = validate_input(&project, &environment, &key);

    if let Err(e) = input_validation_res {
        bail!(e);
    }

    // ok
    let payload = UpdateSecretDescriptionPayload { description };

    let mut spinner = request_spinner();

    let res = secrets::update_description(api_key, project, environment, key, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(_) => {
            spinner.stop_with_message(&format!(
                "{} {}",
                "✓".green(),
                "Description has been updated!"
            ));
        }
        PostPatchRequestApiResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}

fn validate_input(project: &str, environment: &str, key: &str) -> Result<()> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        bail!(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        bail!(err);
    }

    let key_valid = validate_secret_key(&key);

    if let Err(err) = key_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    Ok(())
}
