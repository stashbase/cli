use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::environments,
    models::api_client::PostPatchRequestApiResponse,
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

pub async fn handle_set_env_lock(
    token: String,
    project: String,
    environment: String,
    lock: bool,
) -> Result<()> {
    let input_valid = validate_project_environment(&project, &environment);

    if let Err(err) = input_valid {
        bail!(err);
    }

    // OK
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
