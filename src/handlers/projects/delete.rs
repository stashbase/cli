use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::projects, models::api_client::DeleteRequestApiResponse, utils::spinner::request_spinner,
};

pub async fn handle_delete_project(token: String, name: String) -> Result<()> {
    debug!("deleting project...:");

    let mut spinner = request_spinner();
    let project_res = projects::delete_project(token, name).await;

    spinner.stop_and_persist("", "");

    if let Err(err) = project_res {
        error!("{:#?}", &err);
        bail!(err);
    }

    let project_res = project_res.unwrap();

    match project_res {
        DeleteRequestApiResponse::Ok => {
            println!("Project has been deleted");
        }
        DeleteRequestApiResponse::Err(e) => {
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
