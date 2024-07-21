use std::path::Path;

use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    cmd::{environments::EnvironmentType, secrets::SecretsFileFormat},
    handlers::environments::open::GetEnvUrlResponse,
    models::{
        api_client::PostPatchRequestApiResponse,
        environments::{CreatEnvironmentPayload, EnvType},
        secrets::Secret,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        files::check_file_exists,
        secrets::{parse_secrets_from_str, read_secrets_from_file},
        spinner::request_spinner,
        validation::validate_project_environment,
    },
};

pub struct HandleCreateEnvironmentArgs {
    pub api_key: String,
    pub project: String,
    pub name: String,
    pub env_type: EnvironmentType,
    pub description: Option<String>,
    pub open: bool,
    pub file_path: Option<String>,
    pub format: Option<SecretsFileFormat>,
}

pub async fn handle_create_environment(args: HandleCreateEnvironmentArgs) -> Result<()> {
    let HandleCreateEnvironmentArgs {
        api_key,
        project,
        name,
        env_type,
        description,
        file_path,
        format,
        open,
    } = args;

    let input_valid = validate_project_environment(&project, &name, true);

    if let Err(err) = input_valid {
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

        //
        let target_format = match format {
            Some(format) => format,
            None => {
                if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
                    SecretsFileFormat::Yaml
                } else if file_path.ends_with(".json") {
                    SecretsFileFormat::Json
                } else {
                    SecretsFileFormat::Dotenv
                }
            }
        };

        let secrets_res = read_secrets_from_file(path, &target_format);

        match secrets_res {
            Ok(values) => {
                debug!("{:#?}", values);

                if values.is_empty() {
                    let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found");
                    eprintln!("{}", msg);

                    return Ok(());
                }

                secrets = Some(values);
            }
            Err(e) => {
                let err = InputValidationError::Secrets(SecretsInputValidationError::ReadFile(e));
                bail!(err);
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

    let project_res = environments::create(api_key, project, open, &data).await;

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
