use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::environments,
    models::api_client::PostPatchRequestApiResponse,
    utils::{
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name},
    },
};

pub async fn handle_set_env_lock(
    token: String,
    project: String,
    environment: String,
    lock: bool,
) -> Result<()> {
    // TODO: check valid name
    let project_name_is_valid = validate_project_name(&project, false);

    if let Err(err) = project_name_is_valid {
        bail!(err);
    }

    let env_name_is_valid = validate_environment_name(&environment);
    if let Err(err) = env_name_is_valid {
        bail!(err);
    }

    debug!("updating lock status...:");
    let mut spinner = request_spinner();

    let res = environments::set_lock(token, project, environment, lock).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(format!("Error sending request: {}", err));
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(_) => {
            if lock == true {
                spinner.stop_with_message("🔒 Enviroment locked!");
            } else {
                spinner.stop_with_message("🔓 Enviroment unlocked!");
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
