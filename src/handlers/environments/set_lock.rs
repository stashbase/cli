use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::environments,
    models::api_client::RequestApiOptionResponse,
    utils::{spinner::request_spinner, validation::validate_project_environment_identifier},
};

pub async fn handle_set_env_lock(
    api_key: String,
    project: String,
    environment: String,
    lock: bool,
) -> Result<()> {
    let input_validation_res =
        validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_validation_res {
        bail!(err);
    }

    // OK
    debug!("updating lock status...:");
    let mut spinner = request_spinner();

    let res = environments::set_lock(api_key, project, environment, lock).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            if lock == true {
                spinner.stop_with_message("🔒 Enviroment locked!");
            } else {
                spinner.stop_with_message("🔓 Enviroment unlocked!");
            }
        }
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
