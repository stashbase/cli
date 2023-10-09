use std::path::Path;

use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    cmd::environments::EnvironmentType,
    handlers::environments::open::GetEnvUrlResponse,
    models::{
        api_client::PostPatchRequestApiResponse,
        environments::{CreatEnvironmentPayload, EnvType},
        secrets::Secret,
    },
    utils::{
        files::check_file_exists, secrets::read_dotenv_file, spinner::request_spinner,
        validation::validate_project_name,
    },
};

pub struct HandleCreateEnvironmentArgs {
    pub token: String,
    pub project: String,
    pub name: String,
    pub env_type: EnvironmentType,
    pub description: Option<String>,
    pub open: bool,
    pub file_path: Option<String>,
}

pub async fn handle_create_environment(args: HandleCreateEnvironmentArgs) -> Result<()> {
    let HandleCreateEnvironmentArgs {
        token,
        project,
        name,
        env_type,
        description,
        file_path,
        open,
    } = args;

    // TODO: validate also env name
    let name_is_valid = validate_project_name(&name, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    let mut secrets: Option<Vec<Secret>> = None;

    if let Some(file_path) = file_path {
        let path = Path::new(&file_path);
        let file_exists = check_file_exists(&path);

        if !file_exists {
            let err_msg = format!("{} {}", "Error reading file:".red(), "file does not exist");
            bail!("{}", err_msg);
        }

        let secrets_res = read_dotenv_file(path);

        match secrets_res {
            Ok(values) => {
                debug!("{:#?}", values);

                secrets = Some(values);
            }
            Err(e) => {
                bail!(format!("{} {}", "Error reading file:".red(), e));
            }
        }
    }

    debug!("creating project...:");

    let environment_type: EnvType = env_type.into();

    let data = CreatEnvironmentPayload {
        name,
        description,
        env_type: environment_type,
        secrets,
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
