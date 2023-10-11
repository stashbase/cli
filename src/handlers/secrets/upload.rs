use std::path::Path;

use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::api_client::PostPatchRequestApiResponse,
    utils::{
        secrets::read_dotenv_file, spinner::request_spinner,
        validation::validate_project_environment,
    },
};

pub struct HandleUploadSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub file_path: String,
}

pub async fn handle_upload_secrets(args: HandleUploadSecretsArgs) -> Result<()> {
    let HandleUploadSecretsArgs {
        token,
        project,
        environment,
        file_path,
    } = args;

    let proj_env_validation_res = validate_project_environment(&project, &environment, false);

    if let Err(err) = proj_env_validation_res {
        bail!(err);
    }

    let path = Path::new(&file_path);
    debug!("Path: {:#?}", path);

    let file_exists = path.exists();
    debug!("File exists: {}", file_exists);

    if !file_exists {
        let err_msg = format!("{} {}", "Error reading file:".red(), "file does not exist");
        bail!("{}", err_msg);
    }

    let secrets_res = read_dotenv_file(path);

    match secrets_res {
        Ok(secrets) => {
            let mut spinner = request_spinner();
            let res = secrets::set_sercrets(token, project, environment, &secrets).await;
            debug!("{:#?}", res);

            if let Err(err) = res {
                spinner.stop_and_persist("", "");
                debug!("Error: {:#?}", &err);
                bail!(err);
            }

            let res = res.unwrap();

            match res {
                PostPatchRequestApiResponse::Ok(_) => {
                    spinner.stop_with_message(&format!(
                        "{} {}",
                        "✓".green(),
                        "Secrets have been uploaded!"
                    ));
                }
                PostPatchRequestApiResponse::Err(e) => {
                    debug!("Error: {}", e);
                    spinner.stop_with_message(&format!("{}", e));
                }
            }
        }
        Err(e) => {
            bail!(format!("{} {}", "Error reading file:".red(), e));
        }
    }

    Ok(())
}
