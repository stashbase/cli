use std::path::Path;

use crate::{
    api::secrets,
    cmd::{config::SecretsOutputFormat, push::PushFormat, secrets::SecretsFileFormat},
    handlers::pull::entry::load_from_file,
    models::{
        api_client::RequestApiOptionResponse,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction,
        secrets::{find_duplicate_keys, read_secrets_from_file},
        validation::{
            validate_project_environment_identifier, validate_secrets_references_with_existence,
        },
    },
};
use anyhow::{bail, Context, Result};
use log::debug;
use owo_colors::OwoColorize;
use spinoff::{spinners, Color, Spinner, Streams};

#[derive(Debug)]
pub struct HandlePushArgs {
    pub api_key: String,

    pub config_file_path: Option<String>,
    //
    pub input_file_path: Option<String>,
    pub format: Option<PushFormat>,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
}

pub async fn handle_push(args: HandlePushArgs) -> Result<()> {
    let HandlePushArgs {
        api_key,
        mut format,
        mut only,
        mut exclude,
        config_file_path,
        mut input_file_path,
    } = args;

    let file_config = load_from_file(config_file_path.clone())?;
    debug!("file_config: {:?}", file_config);

    let mut project: Option<String> = None;
    let mut environment: Option<String> = None;

    if let Some(config) = file_config {
        debug!("config: {:?}", config);

        project = Some(config.project);
        environment = Some(config.environment);

        match (config.pull, config.target) {
            (None, None) => {
                bail!("No target or push config for selected environment");
            }
            (Some(pull_config), None) => {
                // check format
                if let None = format {
                    format = pull_config.format;
                }

                if let None = input_file_path {
                    input_file_path = Some(pull_config.file);
                }
            }
            (None, Some(target)) => {
                if let None = format {
                    format = target.format;
                }

                if let None = input_file_path {
                    input_file_path = Some(target.file);
                }
            }
            (Some(_), Some(target)) => {
                if let None = format {
                    format = target.format;
                }

                if let None = input_file_path {
                    input_file_path = Some(target.file);
                }
            }
        }

        // if let Some(secrets) = config.secrets {
        //     // only
        //     if let Some(only_val) = secrets.only {
        //         if only_val.is_empty() == false {
        //             for only_secret in only_val {
        //                 let already_exists = only.contains(&only_secret);
        //
        //                 if !already_exists {
        //                     only.push(only_secret);
        //                 }
        //             }
        //         }
        //     }
        //
        //     // exclude
        //     if let Some(exclude_val) = secrets.exclude {
        //         if exclude_val.is_empty() == false {
        //             for exclude_secret in exclude_val {
        //                 let already_exists = exclude.contains(&exclude_secret);
        //
        //                 if !already_exists {
        //                     exclude.push(exclude_secret);
        //                 }
        //             }
        //         }
        //     }
        // }
    } else {
        return Ok(());
    }

    let input_path = input_file_path.unwrap();
    let path = Path::new(&input_path);

    let file_exists = path.exists();

    if !file_exists {
        let err_msg = format!(
            "{} {}",
            "Error reading input file:".red(),
            "file does not exist"
        );
        bail!("{}", err_msg);
    }

    //
    let target_format = match format {
        Some(format) => {
            // TODO: ???
            SecretsFileFormat::Dotenv
        }
        None => {
            if input_path.ends_with(".yaml") || input_path.ends_with(".yml") {
                SecretsFileFormat::Yaml
            } else if input_path.ends_with(".json") {
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

    let info = format!("\nNumber of screts to push: {}", secrets.len());
    eprintln!("{}", info);

    let confirm = interaction::confirm_opt("Are you sure you want to continue?");

    if confirm.is_none() || (confirm.unwrap() == false) {
        return Ok(());
    }
    eprintln!();

    let project = project.unwrap();
    let environment = environment.unwrap();

    let validation_res =
        validate_project_environment_identifier(project.as_ref(), environment.as_ref(), true);

    if let Err(e) = validation_res {
        bail!(e);
    }

    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Pushing secrets...",
        Color::Cyan,
        Streams::Stderr,
    );

    // file
    let res = secrets::set_sercrets(api_key, project, environment, &secrets).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            spinner.stop_with_message(&format!("{} {}", "✓".green(), "Secrets have been pushed!"));
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
