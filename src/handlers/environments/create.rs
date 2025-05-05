use std::path::Path;

use anyhow::{bail, Result};
use colored_json::{to_colored_json, to_colored_json_auto};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    cmd::secrets::SecretsFileFormat,
    handlers::environments::open::GetEnvUrlResponse,
    models::{
        api_client::{OutputError, RequestApiOptionResponse},
        environments::{CreatEnvironmentPayload, CreateEnvironmentResponse},
        secrets::Secret,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        files::check_file_exists,
        interaction,
        output::get_colored_json,
        secrets::{parse_secrets_from_str, read_secrets_from_file},
        spinner::request_spinner,
        validation::{
            validate_project_environment, validate_secrets_references,
            validate_secrets_references_with_existence,
        },
    },
};

pub struct HandleCreateEnvironmentArgs {
    pub api_key: String,
    pub project: String,
    pub name: String,
    pub is_production: Option<bool>,
    pub description: Option<String>,
    pub open: bool,
    pub file_path: Option<String>,
    pub format: Option<SecretsFileFormat>,
    pub json_format: bool,
}

pub async fn handle_create_environment(args: HandleCreateEnvironmentArgs) -> Result<()> {
    let HandleCreateEnvironmentArgs {
        api_key,
        project,
        name,
        is_production,
        description,
        file_path,
        format,
        open,
        json_format,
    } = args;

    let input_valid = validate_project_environment(&project, &name, true);

    if let Err(err) = input_valid {
        let formatted_err = err.format_error_output(json_format)?;

        eprintln!();
        bail!(formatted_err);
    }

    let mut secrets: Option<Vec<Secret>> = None;

    if let Some(file_path) = file_path {
        let path = Path::new(&file_path);
        let file_exists = check_file_exists(&path);

        if !file_exists {
            if json_format {
                let error_json = serde_json::json!({
                    "error": {
                        "message": "Error reading file: file does not exist."
                    }
                });
                let pretty = to_colored_json_auto(&error_json).unwrap();

                eprintln!();
                bail!(pretty);
            } else {
                let err_msg = format!("{} {}", "Error reading file:".red(), "file does not exist.");
                eprintln!();
                bail!(err_msg);
            }
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
                    let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found.");
                    eprintln!("{}", msg);

                    let confirm = interaction::confirm_opt("Are you sure you want to continue?");

                    if confirm.is_none() || (confirm.unwrap() == false) {
                        return Ok(());
                    }
                } else {
                    let validation_msg = validate_secrets_input(&values)?;

                    if let Some(msg) = validation_msg {
                        eprintln!("{}", msg);
                    }

                    let info = format!("Number of secrets to create: {}", values.len());
                    eprintln!("{}", info);

                    let confirm = interaction::confirm_opt("Are you sure you want to continue?");

                    if confirm.is_none() || (confirm.unwrap() == false) {
                        return Ok(());
                    }

                    eprintln!();
                }

                secrets = Some(values);
            }
            Err(e) => {
                let error = InputValidationError::Secrets(SecretsInputValidationError::ReadFile(
                    e.to_string(),
                ));
                let formatted_err = error.format_error_output(json_format)?;

                eprintln!();
                bail!(formatted_err);
            }
        }
    }

    debug!("creating project...:");

    let data = CreatEnvironmentPayload {
        name,
        description,
        is_production,
        secrets,
    };

    let mut spinner = request_spinner();

    let project_res = environments::create(api_key, project, open, &data).await;

    if let Err(err) = project_res {
        spinner.stop_and_persist("", "");

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let project_res = project_res.unwrap();

    match project_res {
        RequestApiOptionResponse::Ok(data) => {
            debug!("{:#?}", data.text);

            if let Some(json) = data.text {
                let res_data = serde_json::from_str::<CreateEnvironmentResponse>(&json);

                match res_data {
                    Ok(data) => {
                        if json_format {
                            let json_str = get_colored_json(&data).unwrap();

                            spinner.stop_and_persist("", "");
                            println!("{}", json_str);

                            return Ok(());
                        }

                        spinner.stop_with_message("Environment created.");
                        eprint!("Id: ");
                        print!("{}\n", data.id);

                        if let Some(dashboard_url) = data.dashboard_url {
                            eprintln!("{}", &format!("\nOpening URL: {}", dashboard_url));

                            if let Err(err) = webbrowser::open(&dashboard_url) {
                                eprintln!("{}", &format!("Error opening URL: {}", err));
                            }
                        }
                    }
                    Err(_) => {
                        spinner.stop_and_persist("", "");

                        let error = OutputError::failed_to_deserialize_response_body();
                        let formatted_err = error.format_error_output(json_format)?;

                        bail!(formatted_err);
                    }
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn validate_secrets_input(secrets: &Vec<Secret>) -> Result<Option<String>> {
    let refs_validation = validate_secrets_references_with_existence(&secrets);

    if !refs_validation.self_referenced_secrets.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::SelfReferences(
            refs_validation.self_referenced_secrets,
        ));

        bail!(err);
    }

    if !refs_validation.invalid_format.is_empty() || !refs_validation.not_found.is_empty() {
        let mut print_str = String::new();

        if !refs_validation.invalid_format.is_empty() {
            let hint_str = refs_validation
                .invalid_format
                .iter()
                .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            print_str.push_str(&format!("  Message: Invalid secret references format.\n"));
            print_str.push_str(&format!("  Secrets: {} \n", hint_str));
        }

        if !refs_validation.not_found.is_empty() {
            let hint_str = refs_validation
                .not_found
                .iter()
                .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            if !print_str.is_empty() {
                print_str.push_str(&format!("\n"));
            }

            print_str.push_str(&format!(
                "  Message: Referenced secrets not found within the file.\n"
            ));
            print_str.push_str(&format!("  Secret: {} \n", hint_str));
        }

        if !refs_validation.invalid_format.is_empty() && !refs_validation.not_found.is_empty() {
            print_str = format!("{}\n{}", format!("Input warnings").yellow(), print_str);
        } else {
            print_str = format!("{}\n{}", format!("Input warning").yellow(), print_str);
        }

        print_str = format!("{}\n", print_str);
        return Ok(Some(print_str));
    }

    Ok(None)
}
