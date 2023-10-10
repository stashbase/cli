use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{api_client::PostPatchRequestApiResponse, secrets::UpdateSecretDescriptionPayload},
    utils::{
        spinner::request_spinner,
        validation::{validate_project_name, validate_secret_key},
    },
};

pub struct HandleDescriptionArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub key: String,
    pub description: String,
}

pub async fn handle_update_description(args: HandleDescriptionArgs) -> Result<()> {
    let HandleDescriptionArgs {
        token,
        project,
        environment,
        description,
        key,
    } = args;

    // TODO: validation

    let project_name_is_valid = validate_project_name(&project, false);

    if let Err(err) = project_name_is_valid {
        bail!(err);
    }

    let key_valid = validate_secret_key(&key);

    if let Err(err) = key_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    //
    let payload = UpdateSecretDescriptionPayload { description };

    let mut spinner = request_spinner();

    let res = secrets::update_description(token, project, environment, key, &payload).await;

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
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
