use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::secrets,
    models::{
        api_client::GetRequestApiResponse,
        secrets::{
            ProjectSecretSearchedByName, ProjectSecretSearchedByNameTable,
            SecretsSearchOutputFormat,
        },
    },
    utils::{spinner::request_spinner, tables, validation::validate_secret_name},
};

pub struct HandleSearchProjectSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub format: SecretsSearchOutputFormat,
    pub name: Option<String>,
    pub value: Option<String>,
}

pub async fn handle_search_project_secrets(args: HandleSearchProjectSecretsArgs) -> Result<()> {
    let HandleSearchProjectSecretsArgs {
        api_key,
        project,
        format,
        name,
        value,
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

    let mut spinner = request_spinner();
    let res =
        secrets::search_project_secrets_by_name_or_value(api_key, project, &name, &value).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<ProjectSecretSearchedByName>>(&data.text);

            match secrets {
                Ok(secrets) => match format {
                    SecretsSearchOutputFormat::List => {
                        let names = secrets.into_iter().map(|s| s.value).collect::<Vec<_>>();
                        todo!()
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
                },
                Err(e) => bail!("{}", e),
            }
        }
        GetRequestApiResponse::Err(err) => {
            bail!("{}", err);
        }
    }

    todo!()
}
