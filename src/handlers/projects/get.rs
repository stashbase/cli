use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::projects,
    cmd::config::OutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        projects::{
            ProjectWithCountNoDescriptionTable, SingleProject, SingleProjectTable,
            SingleProjectWithCountNoDescriptionTable,
        },
    },
    utils::{
        human_datetime::get_human_datetime,
        spinner::request_spinner,
        tables,
        validation::{validate_project_identifier, validate_project_name},
    },
};

pub async fn handle_get_project(
    api_key: String,
    format: OutputFormat,
    name: String,
    silent: bool,
) -> Result<()> {
    let identifier_is_valid = validate_project_identifier(&name, true);

    if let Err(err) = identifier_is_valid {
        let error_output = err.format_error_output(format == OutputFormat::Json)?;

        eprintln!();
        bail!(error_output);
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = projects::get_project(api_key, name).await;

    if let Err(err) = project_res {
        error!("{:#?}", &err);

        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(format == OutputFormat::Json)?;
        bail!(error_output);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);

            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let project = serde_json::from_str::<SingleProject>(&data.text);

            match project {
                Ok(project) => {
                    debug!("{:#?}", &project);

                    if !silent {
                        match format {
                            OutputFormat::List => {
                                print!("{}", project);
                            }
                            OutputFormat::Json => {
                                let value = serde_json::to_value(&project).unwrap();
                                let pretty = to_colored_json_auto(&value).unwrap();
                                println!("{}", pretty);
                            }
                            OutputFormat::Table => match &project.description {
                                Some(_) => {
                                    let project_item: SingleProjectTable = project.into();

                                    let table =
                                        tables::build::build_table(&Vec::from([project_item]));
                                    println!("{}", table);
                                }
                                None => {
                                    let without_description: SingleProjectWithCountNoDescriptionTable =
                                        project.into();

                                    let table = tables::build::build_table(&Vec::from([
                                        without_description,
                                    ]));
                                    println!("{}", table);
                                }
                            },
                        }
                    }
                }
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(format == OutputFormat::Json)?;

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
    //
    // let project_res = projects::list_projects(api_key).await;
    // spinner.stop_and_persist("", "");
    //
    // if let Err(err) = &project_res {
    //     error!("{:#?}", &err);
    //     bail!("Could not connect to API")
    // }
    //
    // let project_res = project_res.unwrap();
    //
    // let status = project_res.status();
    //
    // if status == 401 {
    //     bail!("Unauthorized")
    // }
    //
    // let response_text = project_res.text().await;
    // debug!("{:#?}", &response_text);
    //
    // match response_text {
    //     Ok(text) => {
    //         let projects = serde_json::from_str::<Vec<Project>>(&text);
    //
    //         match projects {
    //             Ok(projects) => {
    //                 debug!("{:#?}", &projects);
    //                 let value = serde_json::to_value(&projects).unwrap();
    //                 let pretty = to_colored_json_auto(&value).unwrap();
    //
    //                 println!("{}", pretty);
    //             }
    //             Err(e) => {
    //                 error!("{:#?}", &e);
    //                 bail!("Something went wrong")
    //             }
    //         }
    //     }
    //     Err(err) => {
    //         bail!("Could not parse response: {:?}", err);
    //     }
    // }
    //
    // Ok(())
}
