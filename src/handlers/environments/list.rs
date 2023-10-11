use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments,
    cmd::environments::EnvSort,
    models::{api_client::GetRequestApiResponse, environments::Environment},
    utils::{spinner::request_spinner, validation::validate_project_name},
};

pub struct HandleListEnvironmentsArgs {
    pub token: String,
    pub project: String,
    pub sort: Option<EnvSort>,
    pub descending: bool,
    pub raw: bool,
}

pub async fn handle_list_environments(args: HandleListEnvironmentsArgs) -> Result<()> {
    let HandleListEnvironmentsArgs {
        token,
        project,
        sort,
        descending,
        raw,
    } = args;

    let name_is_valid = validate_project_name(&project, false, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    debug!("listing environments...:");

    let mut spinner = request_spinner();
    let env_res =
        environments::list(token, project, sort.unwrap_or(EnvSort::Created), descending).await;

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
