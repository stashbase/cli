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
        api_client::{GetApiResponseOk, GetRequestApiResponse, OutputError},
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
    pub silent: bool,
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
        silent,
    } = args;

    if let Some(project) = &project {
        let validation_res = validate_project_identifier(project, false);

        if let Err(err) = validation_res {
            let error_output =
                err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }
    }

    if name.is_none() && value.is_none() {
        let search_error = SecretsInputValidationError::SearchMissingNameOrValue;
        let err = InputValidationError::Secrets(search_error);
        let error_output = err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    if name.is_some() && value.is_some() {
        let search_error = SecretsInputValidationError::SearchBothNameAndValue;
        let err = InputValidationError::Secrets(search_error);
        let error_output = err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    if let Some(name) = &name {
        if name.is_empty() {
            let search_error = SecretsInputValidationError::SearchTooShort;
            let err = InputValidationError::Secrets(search_error);
            let error_output =
                err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        let validation_res = validate_secret_name(name);

        if let Err(err) = validation_res {
            let error_output =
                err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }
    }

    if let Some(value) = &value {
        if value.is_empty() {
            let search_error = SecretsInputValidationError::SearchValueEmpty;
            let err = InputValidationError::Secrets(search_error);
            let error_output =
                err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        } else if value.len() > 1000 {
            let search_error = SecretsInputValidationError::SearchValueTooLong;
            let err = InputValidationError::Secrets(search_error);
            let error_output =
                err.format_error_output(format == SecretsSearchOutputFormat::Json)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }
    }

    let search_by_name = name.is_some();

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };
    let res =
        secrets::search_secrets(api_key, &project, &name, &value, show_values, with_ids).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(format == SecretsSearchOutputFormat::Json)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => match project {
            Some(_) => match search_by_name {
                true => handle_search_secrets_response::<
                    ProjectSecretSearchedByName,
                    ProjectSecretSearchedByNameTable,
                >(spinner, format, data, silent)?,
                false => handle_search_secrets_response::<
                    ProjectSecretSearchedByValue,
                    ProjectSecretSearchedByValueTable,
                >(spinner, format, data, silent)?,
            },
            None => match search_by_name {
                true => handle_search_secrets_response::<
                    WorkspaceSecretSearchedByName,
                    WorkspaceSecretSearchedByNameTable,
                >(spinner, format, data, silent)?,
                false => handle_search_secrets_response::<
                    WorkspaceSecretSearchedByValue,
                    WorkspaceSecretSearchedByValueTable,
                >(spinner, format, data, silent)?,
            },
        },
        GetRequestApiResponse::Err(err) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output =
                err.format_error_output(format == SecretsSearchOutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn handle_search_secrets_response<SecretType, TableType>(
    spinner: Option<Spinner>,
    format: SecretsSearchOutputFormat,
    data: GetApiResponseOk,
    silent: bool,
) -> Result<()>
where
    SecretType: DeserializeOwned + Serialize + Display,
    TableType: From<SecretType> + Tabled,
{
    let secrets = serde_json::from_str::<Vec<SecretType>>(&data.text);

    match secrets {
        Ok(secrets) => {
            if let SecretsSearchOutputFormat::Json = format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let value = serde_json::to_value(&secrets).unwrap();
                let pretty = to_colored_json_auto(&value).unwrap();
                println!("{}", pretty);

                return Ok(());
            } else if let SecretsSearchOutputFormat::Yaml = format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let value = serde_yaml::to_string(&secrets).unwrap();
                print!("{}", value);

                return Ok(());
            } else if secrets.is_empty() {
                if let Some(mut spinner) = spinner {
                    if !silent {
                        spinner.stop_with_message("No secrets found.");
                    } else {
                        spinner.stop_and_persist("", "");
                    }
                } else if !silent {
                    println!("No secrets found.");
                }
                return Ok(());
            }

            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

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
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error = OutputError::failed_to_deserialize_response_body();
            let formatted_err =
                error.format_error_output(format == SecretsSearchOutputFormat::Json)?;

            bail!(formatted_err);
        }
    }

    Ok(())
}
