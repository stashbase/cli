use std::fmt::Display;

use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;
use serde::{de::DeserializeOwned, Serialize};
use spinoff::Spinner;
use tabled::Tabled;

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
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        spinner::request_spinner,
        tables,
        validation::{validate_project_identifier, validate_secret_name},
    },
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

    if let Some(project) = &project {
        let validation_res = validate_project_identifier(project, false);

        if let Err(err) = validation_res {
            bail!(err);
        }
    }

    if name.is_none() && value.is_none() {
        let search_error = SecretsInputValidationError::SearchMissingNameOrValue;
        let err = InputValidationError::Secrets(search_error);

        bail!(err);
    }

    if name.is_some() && value.is_some() {
        let search_error = SecretsInputValidationError::SearchBothNameAndValue;
        let err = InputValidationError::Secrets(search_error);

        bail!(err);
    }

    if let Some(name) = &name {
        if name.is_empty() {
            let search_error = SecretsInputValidationError::SearchTooShort;
            let err = InputValidationError::Secrets(search_error);

            bail!(err);
        }

        let validation_res = validate_secret_name(name);

        if let Err(err) = validation_res {
            bail!(err);
        }
    }

    if let Some(value) = &value {
        if value.is_empty() {
            let search_error = SecretsInputValidationError::SearchValueEmpty;
            let err = InputValidationError::Secrets(search_error);

            bail!(err);
        } else if value.len() > 1000 {
            let search_error = SecretsInputValidationError::SearchValueTooLong;
            let err = InputValidationError::Secrets(search_error);

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
            Some(_) => match search_by_name {
                true => handle_search_secrets_response::<
                    ProjectSecretSearchedByName,
                    ProjectSecretSearchedByNameTable,
                >(&mut spinner, format, data)?,
                false => handle_search_secrets_response::<
                    ProjectSecretSearchedByValue,
                    ProjectSecretSearchedByValueTable,
                >(&mut spinner, format, data)?,
            },
            None => match search_by_name {
                true => handle_search_secrets_response::<
                    WorkspaceSecretSearchedByName,
                    WorkspaceSecretSearchedByNameTable,
                >(&mut spinner, format, data)?,
                false => handle_search_secrets_response::<
                    WorkspaceSecretSearchedByValue,
                    WorkspaceSecretSearchedByValueTable,
                >(&mut spinner, format, data)?,
            },
        },
        GetRequestApiResponse::Err(err) => {
            spinner.stop_and_persist("", "");
            bail!("{}", err);
        }
    }

    Ok(())
}

fn handle_search_secrets_response<SecretType, TableType>(
    spinner: &mut Spinner,
    format: SecretsSearchOutputFormat,
    data: GetApiResponseOk,
) -> Result<()>
where
    SecretType: DeserializeOwned + Serialize + Display,
    TableType: From<SecretType> + Tabled,
{
    let secrets = serde_json::from_str::<Vec<SecretType>>(&data.text);

    match secrets {
        Ok(secrets) => {
            if let SecretsSearchOutputFormat::Json = format {
                spinner.stop_and_persist("", "");

                let value = serde_json::to_value(&secrets).unwrap();
                let pretty = to_colored_json_auto(&value).unwrap();
                println!("{}", pretty);

                return Ok(());
            } else if let SecretsSearchOutputFormat::Yaml = format {
                spinner.stop_and_persist("", "");

                let value = serde_yaml::to_string(&secrets).unwrap();
                print!("{}", value);

                return Ok(());
            } else if secrets.is_empty() {
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

                    print!("{}", str);
                }
                SecretsSearchOutputFormat::Table => {
                    let table_items = secrets
                        .into_iter()
                        .map(|s| s.into())
                        .collect::<Vec<TableType>>();

                    let table = tables::build::build_table(&table_items);
                    println!("{}", table);
                }
                _ => unreachable!(),
            }
        }
        Err(_) => {
            spinner.stop_and_persist("", "");
            bail!("Something went wrong")
        }
    }

    Ok(())
}
