use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::environments,
    cmd::config::OutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        environments::{
            Environment, TableEnvironment, TableEnvironmentWithProject,
            TableEnvironmentWithProjectWithoutDescription, TableEnvironmentWithoutDescription,
        },
    },
    utils::{
        output::get_formatted_json_string, spinner::request_spinner, tables,
        validation::validate_project_environment_identifier,
    },
};

pub async fn handle_get_environment(
    api_key: String,
    format: OutputFormat,
    silent: bool,
    project: Option<String>,
    environment: Option<String>,
) -> Result<()> {
    if project.is_some() && environment.is_some() {
        let input_valid = validate_project_environment_identifier(
            project.as_ref().unwrap(),
            environment.as_ref().unwrap(),
            true,
        );

        if let Err(err) = input_valid {
            let formatted_err = err.format_error_output(format == OutputFormat::Json)?;

            if !silent {
                eprintln!();
            }

            bail!(formatted_err);
        }
    }

    // OK
    debug!("getting env...");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = environments::get(api_key, project, environment).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(format == OutputFormat::Json)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);

            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let environment = serde_json::from_str::<Environment>(&data.text);

            match environment {
                Ok(env) => {
                    debug!("{:#?}", &env);

                    match format {
                        OutputFormat::Json => {
                            let pretty = get_formatted_json_string(&env, true).unwrap();
                            println!("{}", pretty);
                        }
                        OutputFormat::Plain => {
                            print!("{}", env);
                        }
                        OutputFormat::Table => match env.description {
                            Some(_) => {
                                if let Some(_) = &env.project {
                                    let table_env: TableEnvironmentWithProject = env.into();

                                    let table = tables::build::build_table(&vec![table_env]);
                                    println!("{}", table);
                                } else {
                                    let table_env: TableEnvironment = env.into();

                                    let table = tables::build::build_table(&vec![table_env]);
                                    println!("{}", table);
                                }
                            }
                            None => {
                                if let Some(_) = &env.project {
                                    let table_env: TableEnvironmentWithProjectWithoutDescription =
                                        env.into();

                                    let table = tables::build::build_table(&vec![table_env]);
                                    println!("{}", table);
                                } else {
                                    let table_env: TableEnvironmentWithoutDescription = env.into();

                                    let table = tables::build::build_table(&vec![table_env]);
                                    println!("{}", table);
                                }
                            }
                        },
                    }
                }
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(format == OutputFormat::Json)?;

                    if !silent {
                        eprintln!();
                    }

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(format == OutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}
