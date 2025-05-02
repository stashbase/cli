use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::projects,
    cmd::config::OutputFormat,
    models::{
        api_client::GetRequestApiResponse,
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

pub async fn handle_get_project(api_key: String, format: OutputFormat, name: String) -> Result<()> {
    let identifier_is_valid = validate_project_identifier(&name, true);

    if let Err(err) = identifier_is_valid {
        bail!(err);
    }

    let mut spinner = request_spinner();
    let project_res = projects::get_project(api_key, name).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            spinner.stop_and_persist("", "");

            let project = serde_json::from_str::<SingleProject>(&data.text);

            match project {
                Ok(project) => {
                    debug!("{:#?}", &project);

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

                                let table = tables::build::build_table(&Vec::from([project_item]));
                                println!("{}", table);
                            }
                            None => {
                                let without_description: SingleProjectWithCountNoDescriptionTable =
                                    project.into();

                                let table =
                                    tables::build::build_table(&Vec::from([without_description]));
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
            bail!("{}", e);
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
