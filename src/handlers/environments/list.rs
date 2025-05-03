use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments::{self, ListEnvsRequestArgs},
    cmd::{config::OutputFormat, environments::EnvSortBy},
    models::{
        api_client::GetRequestApiResponse,
        environments::{Environment, TableEnvironment, TableEnvironmentWithoutDescription},
    },
    utils::{
        spinner::request_spinner,
        tables,
        validation::{validate_env_search, validate_project_identifier, validate_project_name},
    },
};

pub struct HandleListEnvironmentsArgs {
    pub api_key: String,
    pub project: String,
    pub search: Option<String>,
    pub sort_by: Option<EnvSortBy>,
    pub descending: bool,
    pub is_production: Option<bool>,
    pub locked: bool,
    pub unlocked: bool,
    pub format: OutputFormat,
}

pub async fn handle_list_environments(args: HandleListEnvironmentsArgs) -> Result<()> {
    let HandleListEnvironmentsArgs {
        api_key,
        project,
        search,
        sort_by: sort,
        descending,
        is_production,
        locked,
        unlocked,
        format,
    } = args;

    debug!("{:#?}", is_production);

    let project_identifier_vlidation_result = validate_project_identifier(&project, false);

    if let Err(err) = project_identifier_vlidation_result {
        eprintln!("");
        bail!(err);
    }

    // validate search
    if let Some(search) = &search {
        let search_validation_res = validate_env_search(&search);

        if let Err(err) = search_validation_res {
            eprintln!("");
            bail!(err);
        }
    }

    debug!("listing environments...:");

    let mut spinner = request_spinner();

    let args = ListEnvsRequestArgs {
        api_key,
        project,
        is_production,
        locked,
        unlocked,
        search,
        sort_by: sort.unwrap_or_default(),
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

                    if let OutputFormat::Json = format {
                        spinner.stop_and_persist("", "");
                        let value = serde_json::to_value(&envs).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    } else {
                        if envs.is_empty() {
                            spinner.stop_with_message("No environments found.");
                            return Ok(());
                        } else {
                            spinner.stop_and_persist("", "");
                        }

                        match format {
                            OutputFormat::List => {
                                for (i, p) in envs.iter().enumerate() {
                                    if i == envs.len() - 1 {
                                        print!("{}", p);
                                    } else {
                                        println!("{}", p);
                                    }
                                }
                            }
                            OutputFormat::Table => {
                                let has_description = envs.iter().any(|e| e.description.is_some());

                                if has_description {
                                    let table_envs: Vec<_> = envs
                                        .into_iter()
                                        .map(|env| TableEnvironment::from(env))
                                        .collect();

                                    let table = tables::build::build_table(&table_envs);
                                    println!("{}", table);
                                } else {
                                    let table_envs: Vec<_> = envs
                                        .into_iter()
                                        .map(|env| TableEnvironmentWithoutDescription::from(env))
                                        .collect();

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
                    bail!("Something went wrong.")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
        }
    }

    Ok(())
}
