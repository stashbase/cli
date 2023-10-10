use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments,
    models::{api_client::GetRequestApiResponse, environments::Environment},
    utils::{
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name},
    },
};

pub async fn handle_get_environment(
    token: String,
    raw: bool,
    project: String,
    environment: String,
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

    debug!("getting env...");

    let mut spinner = request_spinner();
    let res = environments::get(token, project, environment).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            spinner.stop_and_persist("", "");

            let environment = serde_json::from_str::<Environment>(&data.text);

            match environment {
                Ok(project) => {
                    debug!("{:#?}", &project);

                    if raw {
                        let value = serde_json::to_value(&project).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();
                        println!("{}", pretty);
                    } else {
                        print!("{}", project);
                    }
                }
                Err(_) => {
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
