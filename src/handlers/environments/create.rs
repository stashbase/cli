use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::environments,
    cmd::environments::EnvironmentType,
    handlers::environments::open::GetEnvUrlResponse,
    models::{
        api_client::PostPatchRequestApiResponse,
        environments::{CreatEnvironmentPayload, EnvType},
    },
    utils::{spinner::request_spinner, validation::validate_project_name},
};

pub async fn handle_create_environment(
    token: String,
    project: String,
    name: String,
    env_type: EnvironmentType,
    description: Option<String>,
    open: bool,
) -> Result<()> {
    // TODO: validate also env name
    let name_is_valid = validate_project_name(&name, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    debug!("creating project...:");

    let environment_type: EnvType = env_type.into();

    let data = CreatEnvironmentPayload {
        name,
        description,
        env_type: environment_type,
    };

    let mut spinner = request_spinner();

    let project_res = environments::create(token, project, open, data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");
        error!("{:#?}", &err);
        bail!(format!("Error sending request: {}", err));
    }

    let project_res = project_res.unwrap();

    match project_res {
        PostPatchRequestApiResponse::Ok(data) => {
            spinner.stop_with_message("🔥 Environment created!");

            debug!("{:#?}", data.text);

            if let Some(json) = data.text {
                let res_data = serde_json::from_str::<GetEnvUrlResponse>(&json);

                match res_data {
                    Ok(data) => {
                        let url = data.url;

                        eprintln!("{}", &format!("Opening URL: {}", url));

                        if let Err(err) = webbrowser::open(&url) {
                            eprintln!("{}", &format!("Error opening URL: {}", err));
                        }
                    }
                    Err(_) => {
                        bail!("Something went wrong when when opening environment");
                    }
                }
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            // spinner.stop_and_persist("", "");
            // eprint!("{}", e);
            // error!("{:#?}", &e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
