use anyhow::bail;
use colored_json::to_colored_json_auto;
use log::{debug, error};

use crate::{
    api::projects,
    models::{api_client::GetRequestApiResponse, projects::Project},
    utils::spinner::request_spinner,
};

pub async fn handle_list_projects(token: String) -> anyhow::Result<()> {
    debug!("listing projects...:");

    let mut spinner = request_spinner();
    let project_res = projects::list_projects(token).await;

    spinner.stop_and_persist("", "");

    if let Err(err) = project_res {
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            let projects = serde_json::from_str::<Vec<Project>>(&data.text);
            match projects {
                Ok(projects) => {
                    debug!("{:#?}", &projects);
                    let value = serde_json::to_value(&projects).unwrap();
                    let pretty = to_colored_json_auto(&value).unwrap();

                    println!("{}", pretty);
                }
                Err(_) => {
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            error!("{:#?}", &e);
            eprint!("{}", e);
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
