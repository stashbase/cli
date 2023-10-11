use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments::{self, ListEnvsRequestArgs},
    cmd::environments::{EnvSort, EnvironmentType},
    models::{api_client::GetRequestApiResponse, environments::Environment},
    utils::{spinner::request_spinner, validation::validate_project_name},
};

pub struct HandleListEnvironmentsArgs {
    pub token: String,
    pub project: String,
    pub sort: Option<EnvSort>,
    pub descending: bool,
    pub types: Vec<EnvironmentType>,
    pub locked: bool,
    pub unlocked: bool,
    pub raw: bool,
}

pub async fn handle_list_environments(args: HandleListEnvironmentsArgs) -> Result<()> {
    let HandleListEnvironmentsArgs {
        token,
        project,
        sort,
        descending,
        types,
        locked,
        unlocked,
        raw,
    } = args;

    debug!("{:#?}", types);

    let name_is_valid = validate_project_name(&project, false, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    debug!("listing environments...:");

    let mut spinner = request_spinner();

    let args = ListEnvsRequestArgs {
        token,
        project,
        types,
        locked,
        unlocked,
        sort: sort.unwrap_or(EnvSort::Created),
        descending,
    };

    let env_res = environments::list(args).await;

    if let Err(err) = env_res {
        spinner.stop_and_persist("", "");
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
                        spinner.stop_and_persist("", "");
                        let value = serde_json::to_value(&envs).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    } else {
                        if envs.is_empty() {
                            spinner.stop_with_message("No environments found");
                        } else {
                            spinner.stop_and_persist("", "");

                            for (i, p) in envs.iter().enumerate() {
                                if i == envs.len() - 1 {
                                    print!("{}", p);
                                } else {
                                    println!("{}", p);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    spinner.stop_and_persist("", "");
                    debug!("Err: {}", e);
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            eprint!("{}", e);
        }
    }

    Ok(())
}
