use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::environments,
    cmd::config::OutputFormat,
    models::{
        api_client::GetRequestApiResponse,
        environments::{Environment, TableEnvironment, TableEnvironmentWithoutDescription},
    },
    utils::{
        spinner::request_spinner,
        tables,
        validation::{validate_project_environment, validate_project_environment_identifier},
    },
};

pub async fn handle_get_environment(
    api_key: String,
    format: OutputFormat,
    project: String,
    environment: String,
) -> Result<()> {
    let input_valid = validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_valid {
        let formatted_err = err.format_error_output(format == OutputFormat::Json)?;

        eprintln!();
        bail!(formatted_err);
    }

    // OK
    debug!("getting env...");

    let mut spinner = request_spinner();
    let res = environments::get(api_key, project, environment).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");

        let error_output = err.format_error_output(format == OutputFormat::Json)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            spinner.stop_and_persist("", "");

            let environment = serde_json::from_str::<Environment>(&data.text);

            match environment {
                Ok(env) => {
                    debug!("{:#?}", &env);

                    match format {
                        OutputFormat::Json => {
                            let value = serde_json::to_value(&env).unwrap();
                            let pretty = to_colored_json_auto(&value).unwrap();
                            println!("{}", pretty);
                        }
                        OutputFormat::List => {
                            print!("{}", env);
                        }
                        OutputFormat::Table => match env.description {
                            Some(_) => {
                                let table_env: TableEnvironment = env.into();

                                let table = tables::build::build_table(&vec![table_env]);
                                println!("{}", table);
                            }
                            None => {
                                let table_env: TableEnvironmentWithoutDescription = env.into();

                                let table = tables::build::build_table(&vec![table_env]);
                                println!("{}", table);
                            }
                        },
                    }
                }
                Err(_) => {
                    bail!("Something went wrong.")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(format == OutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}
