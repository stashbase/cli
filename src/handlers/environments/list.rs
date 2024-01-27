use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments::{self, ListEnvsRequestArgs},
    cmd::environments::{EnvSort, EnvironmentFormat, EnvironmentType},
    models::{
        api_client::GetRequestApiResponse,
        environments::{Environment, TableEnvironment, TableEnvironmentWithoutDescription},
    },
    utils::{
        spinner::request_spinner,
        tables,
        validation::{validate_env_search, validate_project_name},
    },
};

pub struct HandleListEnvironmentsArgs {
    pub api_key: String,
    pub project: String,
    pub search: Option<String>,
    pub sort: Option<EnvSort>,
    pub descending: bool,
    pub types: Vec<EnvironmentType>,
    pub locked: bool,
    pub unlocked: bool,
    pub format: EnvironmentFormat,
}

pub async fn handle_list_environments(args: HandleListEnvironmentsArgs) -> Result<()> {
    let HandleListEnvironmentsArgs {
        api_key,
        project,
        search,
        sort,
        descending,
        types,
        locked,
        unlocked,
        format,
    } = args;

    debug!("{:#?}", types);

    let name_is_valid = validate_project_name(&project, false, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    // validate search
    if let Some(search) = &search {
        let search_validation_res = validate_env_search(&search);

        if let Err(err) = search_validation_res {
            bail!(err);
        }
    }

    debug!("listing environments...:");

    let mut spinner = request_spinner();

    let args = ListEnvsRequestArgs {
        api_key,
        project,
        types,
        locked,
        unlocked,
        search,
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

                    if let EnvironmentFormat::Json = format {
                        spinner.stop_and_persist("", "");
                        let value = serde_json::to_value(&envs).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    } else {
                        if envs.is_empty() {
                            spinner.stop_with_message("No environments found");
                        } else {
                            spinner.stop_and_persist("", "");
                        }

                        match format {
                            EnvironmentFormat::List => {
                                for (i, p) in envs.iter().enumerate() {
                                    if i == envs.len() - 1 {
                                        print!("{}", p);
                                    } else {
                                        println!("{}", p);
                                    }
                                }
                            }
                            EnvironmentFormat::Table => {
                                let has_description = envs.iter().any(|e| e.description.is_some());

                                if has_description {
                                    let mut table_envs: Vec<_> = envs
                                        .into_iter()
                                        .map(|env| TableEnvironment::from(env))
                                        .collect();

                                    table_envs.reverse();

                                    let table = tables::build::build_table(&table_envs);
                                    println!("{}", table);
                                } else {
                                    let mut table_envs: Vec<_> = envs
                                        .into_iter()
                                        .map(|env| TableEnvironmentWithoutDescription::from(env))
                                        .collect();

                                    table_envs.reverse();

                                    let table = tables::build::build_table(&table_envs);
                                    println!("{}", table);
                                }
                            }
                            _ => {}
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
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
