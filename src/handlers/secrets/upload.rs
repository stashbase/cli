use std::{collections::HashMap, path::Path};

use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    models::{
        api_client::PostPatchRequestApiResponse,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction,
        secrets::{find_duplicate_keys, read_secrets_from_file},
        spinner::request_spinner,
        validation::{validate_project_environment, validate_secrets_references_with_existence},
    },
};

pub struct HandleUploadSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub file_path: String,
    pub format: Option<SecretsFileFormat>,
}

pub async fn handle_upload_secrets(args: HandleUploadSecretsArgs) -> Result<()> {
    let HandleUploadSecretsArgs {
        api_key,
        project,
        environment,
        file_path,
        format,
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

    // let secrets_res = read_secrets_file(path);
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

    if let Err(err) = secrets_res {
        let err = InputValidationError::Secrets(SecretsInputValidationError::ReadFile(err));
        bail!(err);
    }

    let secrets = secrets_res.unwrap();

    if secrets.is_empty() {
        let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found");
        eprintln!("{}", msg);

        return Ok(());
    }

    let duplicate_keys = find_duplicate_keys(&secrets);

    if !duplicate_keys.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateKeys(
            duplicate_keys,
        ));

        bail!("{}", err);
    }

    let refs_validation = validate_secrets_references_with_existence(&secrets);

    if !refs_validation.self_referenced_secrets.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::SelfReferences(
            refs_validation.self_referenced_secrets,
        ));
        bail!(err);
    } else if !refs_validation.invalid_format.is_empty() || !refs_validation.not_found.is_empty() {
        let mut print_str = String::new();

        if !refs_validation.invalid_format.is_empty() {
            let hint_str = refs_validation
                .invalid_format
                .iter()
                .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
                .collect::<Vec<_>>()
                .join(", ");

            print_str.push_str(&format!("- message: invalid secret references format\n"));
            print_str.push_str(&format!("- secrets: {} \n", hint_str));
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
                "- message: referenced secrets not found within the file\n"
            ));
            print_str.push_str(&format!("- secret: {} \n", hint_str));
        }

        if !refs_validation.invalid_format.is_empty() && !refs_validation.not_found.is_empty() {
            eprintln!("{}", format!("{}", "Input warnings").yellow());
        } else {
            eprintln!("{}", format!("{}", "Input warning").yellow());
        }
        eprintln!("{}\n", print_str);

        // let hint_str = references_validation
        //     .invalid_format_references
        //     .iter()
        //     .map(|(k, v)| format!("{} ({})", k, v.join(", ")))
        //     .collect::<Vec<_>>()
        //     .join(", ");
        //
        // eprintln!("{}", format!("{}", "Input warning").yellow());
        //
        // eprintln!("- message: invalid secret references");
        // eprintln!("- secret: {} \n", hint_str);
        //
        // let confirm = interaction::confirm_opt("Are you sure you want to continue?");
        //
        // if confirm.is_none() || (confirm.unwrap() == false) {
        //     return Ok(());
        // }
    }

    let info = format!("Number of screts to upload: {}", secrets.len());
    eprintln!("{}", info);

    let confirm = interaction::confirm_opt("Are you sure you want to continue?");

    if confirm.is_none() || (confirm.unwrap() == false) {
        return Ok(());
    }
    eprintln!();

    let mut spinner = request_spinner();
    let res = secrets::set_sercrets(api_key, project, environment, &secrets).await;
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

    Ok(())
}
