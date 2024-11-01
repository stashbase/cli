use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;
use spinoff::Spinner;

use crate::{
    api::secrets,
    models::{
        api_client::{GetApiResponseOk, GetRequestApiResponse},
        secrets::{
            ProjectSecretSearchedByName, ProjectSecretSearchedByNameTable,
            ProjectSecretSearchedByValue, ProjectSecretSearchedByValueTable,
            SecretsSearchOutputFormat, WorkspaceSecretSearchedByName,
            WorkspaceSecretSearchedByNameTable, WorkspaceSecretSearchedByValue,
            WorkspaceSecretSearchedByValueTable,
        },
    },
    utils::{spinner::request_spinner, tables, validation::validate_secret_name},
};

pub struct HandleSearchSecretsArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub format: SecretsSearchOutputFormat,
    pub name: Option<String>,
    pub value: Option<String>,
    pub show_values: bool,
    pub with_ids: bool,
}

pub async fn handle_search_secrets(args: HandleSearchSecretsArgs) -> Result<()> {
    let HandleSearchSecretsArgs {
        api_key,
        project,
        format,
        name,
        value,
        show_values,
        with_ids,
    } = args;

    if name.is_none() && value.is_none() {
        bail!("{}", "Input error: no search criteria provided");
    }

    if name.is_some() && value.is_some() {
        bail!("{}", "Input error: cannot provide both name and value");
    }

    if let Some(name) = &name {
        let validation_res = validate_secret_name(name);

        if let Err(err) = validation_res {
            bail!(err);
        }
    }

    let search_by_name = name.is_some();

    let mut spinner = request_spinner();
    let res =
        secrets::search_secrets(api_key, &project, &name, &value, show_values, with_ids).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => match project {
            Some(_) => {
                handle_search_project_secrets_response(&mut spinner, format, search_by_name, data)?
            }
            None => handle_search_workspace_secrets_response(
                &mut spinner,
                format,
                search_by_name,
                data,
            )?,
        },
        GetRequestApiResponse::Err(err) => {
            spinner.stop_and_persist("", "");
            bail!("{}", err);
        }
    }

    Ok(())
}

fn handle_search_project_secrets_response(
    spinner: &mut Spinner,
    format: SecretsSearchOutputFormat,
    search_by_name: bool,
    data: GetApiResponseOk,
) -> Result<()> {
    if search_by_name == true {
        let secrets = serde_json::from_str::<Vec<ProjectSecretSearchedByName>>(&data.text);

        match secrets {
            Ok(secrets) => {
                if secrets.is_empty() {
                    spinner.stop_with_message("No secrets found");
                    return Ok(());
                }

                spinner.stop_and_persist("", "");

                match format {
                    SecretsSearchOutputFormat::List => {
                        let str = secrets
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join("\n");

                        println!("{}", str);
                    }
                    SecretsSearchOutputFormat::Table => {
                        let table_items = secrets
                            .into_iter()
                            .map(|s| s.into())
                            .collect::<Vec<ProjectSecretSearchedByNameTable>>();

                        let table = tables::build::build_table(&table_items);
                        println!("{}", table);
                    }
                    SecretsSearchOutputFormat::Json => {
                        let value = serde_json::to_value(&secrets).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    }
                }
            }
            Err(_) => {
                spinner.stop_and_persist("", "");
                bail!("Something went wrong")
            }
        }
    } else {
        let secrets = serde_json::from_str::<Vec<ProjectSecretSearchedByValue>>(&data.text);

        match secrets {
            Ok(secrets) => {
                if secrets.is_empty() {
                    spinner.stop_with_message("No secrets found");
                    return Ok(());
                }

                spinner.stop_and_persist("", "");

                match format {
                    SecretsSearchOutputFormat::List => {
                        let str = secrets
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join("\n");

                        println!("{}", str);
                    }
                    SecretsSearchOutputFormat::Table => {
                        let table_items = secrets
                            .into_iter()
                            .map(|s| s.into())
                            .collect::<Vec<ProjectSecretSearchedByValueTable>>();

                        let table = tables::build::build_table(&table_items);
                        println!("{}", table);
                    }
                    SecretsSearchOutputFormat::Json => {
                        let value = serde_json::to_value(&secrets).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    }
                }
            }
            Err(_) => {
                spinner.stop_and_persist("", "");
                bail!("Something went wrong")
            }
        }
    }

    Ok(())
}

fn handle_search_workspace_secrets_response(
    spinner: &mut Spinner,
    format: SecretsSearchOutputFormat,
    search_by_name: bool,
    data: GetApiResponseOk,
) -> Result<()> {
    if search_by_name == true {
        let secrets = serde_json::from_str::<Vec<WorkspaceSecretSearchedByName>>(&data.text);

        match secrets {
            Ok(secrets) => {
                if secrets.is_empty() {
                    spinner.stop_with_message("No secrets found");
                    return Ok(());
                }

                spinner.stop_and_persist("", "");

                match format {
                    SecretsSearchOutputFormat::List => {
                        let str = secrets
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join("\n");

                        println!("{}", str);
                    }
                    SecretsSearchOutputFormat::Table => {
                        let table_items = secrets
                            .into_iter()
                            .map(|s| s.into())
                            .collect::<Vec<WorkspaceSecretSearchedByNameTable>>();

                        let table = tables::build::build_table(&table_items);
                        println!("{}", table);
                    }
                    SecretsSearchOutputFormat::Json => {
                        let value = serde_json::to_value(&secrets).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    }
                }
            }
            Err(_) => {
                spinner.stop_and_persist("", "");
                bail!("Something went wrong")
            }
        }
    } else {
        let secrets = serde_json::from_str::<Vec<WorkspaceSecretSearchedByValue>>(&data.text);

        match secrets {
            Ok(secrets) => {
                if secrets.is_empty() {
                    spinner.stop_with_message("No secrets found");
                    return Ok(());
                }

                spinner.stop_and_persist("", "");

                match format {
                    SecretsSearchOutputFormat::List => {
                        let str = secrets
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                            .join("\n");

                        println!("{}", str);
                    }
                    SecretsSearchOutputFormat::Table => {
                        let table_items = secrets
                            .into_iter()
                            .map(|s| s.into())
                            .collect::<Vec<WorkspaceSecretSearchedByValueTable>>();

                        let table = tables::build::build_table(&table_items);
                        println!("{}", table);
                    }
                    SecretsSearchOutputFormat::Json => {
                        let value = serde_json::to_value(&secrets).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();

                        println!("{}", pretty);
                    }
                }
            }
            Err(_) => {
                spinner.stop_and_persist("", "");
                bail!("Something went wrong")
            }
        }
    }

    Ok(())
}
