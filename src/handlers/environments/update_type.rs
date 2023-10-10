use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::environments,
    cmd::environments::EnvironmentType,
    models::{
        api_client::PostPatchRequestApiResponse,
        environments::{EnvType, UpdateEnvironmentTypePayload},
    },
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

pub async fn handle_update_env_type(
    token: String,
    project: String,
    environment: String,
    env_type: EnvironmentType,
) -> Result<()> {
    let input_valid_res = validate_project_environment(&project, &environment);

    if let Err(err) = input_valid_res {
        bail!(err);
    }

    debug!("updating env type...:");

    let environment_type: EnvType = env_type.into();

    let payload = UpdateEnvironmentTypePayload {
        env_type: environment_type,
    };

    let mut spinner = request_spinner();
    let res = environments::update_type(token, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(format!("Error sending request: {}", err));
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(_) => {
            spinner.stop_with_message("✏️ Environment type been updated!");
        }
        PostPatchRequestApiResponse::Err(e) => {
            // error!("{:#?}", &e);
            // eprint!("{}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
