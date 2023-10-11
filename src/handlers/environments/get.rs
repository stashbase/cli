use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments,
    models::{api_client::GetRequestApiResponse, environments::Environment},
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

pub async fn handle_get_environment(
    token: String,
    raw: bool,
    project: String,
    environment: String,
) -> Result<()> {
    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    // OK
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
