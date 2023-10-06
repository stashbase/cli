use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments,
    models::{api_client::GetRequestApiResponse, environments::Environment},
    utils::{spinner::request_spinner, validation::validate_project_name},
};

pub async fn handle_list_environments(token: String, raw: bool, project: String) -> Result<()> {
    let name_is_valid = validate_project_name(&project, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    debug!("listing environments...:");

    let mut spinner = request_spinner();
    let env_res = environments::list(token, project).await;

    spinner.stop_and_persist("", "");

    if let Err(err) = env_res {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let env_res = env_res.unwrap();

    match env_res {
        GetRequestApiResponse::Ok(data) => {
            let environments = serde_json::from_str::<Vec<Environment>>(&data.text);

            match environments {
                Ok(envs) => {
                    debug!("{:#?}", &envs);

                    if raw {
                        let value = serde_json::to_value(&envs).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    } else {
                        for (i, p) in envs.iter().enumerate() {
                            if i == envs.len() - 1 {
                                print!("{}", p);
                            } else {
                                println!("{}", p);
                            }
                        }
                    }
                }
                Err(_) => {
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            eprint!("{}", e);
        }
    }

    Ok(())
}
