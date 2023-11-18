use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::projects,
    cmd::projects::ProjectsFromat,
    models::{
        api_client::GetRequestApiResponse,
        projects::{
            ProjectWithCount, ProjectWithCountNoDescriptionTable, SingleProject,
            SingleProjectTable, SingleProjectWithCountNoDescriptionTable,
        },
    },
    utils::{
        human_datetime::get_human_datetime, spinner::request_spinner, tables,
        validation::validate_project_name,
    },
};

pub async fn handle_get_project(token: String, format: ProjectsFromat, name: String) -> Result<()> {
    let name_is_valid = validate_project_name(&name, false, true);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    let mut spinner = request_spinner();
    let project_res = projects::get_project(token, name).await;

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
                Ok(mut project) => {
                    debug!("{:#?}", &project);

                    match format {
                        ProjectsFromat::List => {
                            print!("{}", project);
                        }
                        ProjectsFromat::Json => {
                            let value = serde_json::to_value(&project).unwrap();
                            let pretty = to_colored_json_auto(&value).unwrap();
                            println!("{}", pretty);
                        }
                        ProjectsFromat::Table => match &project.description {
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
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            // error!("{:#?}", &e);
            // eprint!("{}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
    //
    // let project_res = projects::list_projects(token).await;
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
