use std::{collections::HashSet, path::Path};

use crate::{
    api::secrets,
    cmd::{pull::PullFormat, push::PushFormat, secrets::SecretsFileFormat},
    handlers::{pull::entry::load_from_file, run::entry::get_set_key_value_pairs},
    models::{
        api_client::RequestApiOptionResponse,
        config_env::{ConfigActionCommand, EnvConfigItem},
        secrets::Secret,
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError,
            PushPullInputValidationError, SecretsInputValidationError,
        },
    },
    utils::{
        interaction,
        secrets::{find_duplicate_keys, read_secrets_from_file},
        validation::{
            validate_project_environment_identifier, validate_secrets_references_with_existence,
        },
    },
};
use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;
use spinoff::{spinners, Color, Spinner, Streams};

#[derive(Debug)]
pub struct HandlePushArgs {
    pub api_key: String,

    pub config_file_path: Option<String>,
    //
    pub target_file: Option<String>,
    pub format: Option<PushFormat>,
    //
    pub only: Vec<String>,
    pub set: Vec<String>,
    pub exclude: Vec<String>,
}

pub async fn handle_push(args: HandlePushArgs) -> Result<()> {
    let HandlePushArgs {
        api_key,
        config_file_path,
        only,
        exclude,
        mut set,
        mut format,
        mut target_file,
    } = args;

    let config_action_command = ConfigActionCommand::Push;

    let selected_config_item =
        EnvConfigItem::select_from_file(config_file_path.clone(), &config_action_command)?;
    debug!("file_config: {:?}", selected_config_item);

    let project: Option<String>;
    let environment: Option<String>;

    let mut only_set: HashSet<_> = only.into_iter().collect();
    let mut exclude_set: HashSet<_> = exclude.into_iter().collect();

    if let Some(config) = selected_config_item {
        debug!("config: {:?}", config);

        if let None = target_file {
            let target_file_path = config.get_push_target_file();
            target_file = target_file_path;
        }

        if let None = target_file {
            let err = InputValidationError::PushPullEnvironment(
                PushPullInputValidationError::NoFileSpecified { is_push: true },
            );

            bail!(err);
        }

        if let None = format {
            let format_config = config.get_push_format();
            format = format_config;
        }

        let secrets_config = config.get_push_secrets();

        if let Some(only_secrets_config) = secrets_config.only {
            for only_secret in only_secrets_config {
                only_set.insert(only_secret);
            }
        }

        if let Some(exclude_secrets_config) = secrets_config.exclude {
            for exclude_secret in exclude_secrets_config {
                exclude_set.insert(exclude_secret);
            }
        }

        // set
        if let Some(set_val) = secrets_config.set {
            if set_val.is_empty() == false {
                let mut set_secrets_from_file = Vec::new();

                for (key, value) in set_val {
                    let key_value_str = format!("{}={}", key, value);

                    if set.contains(&key_value_str) == false {
                        set_secrets_from_file.push(key_value_str);
                    }
                }

                set = [set_secrets_from_file, set].concat();
            }
        }

        project = Some(config.project);
        environment = Some(config.environment);
    } else {
        return Ok(());
    }

    let input_path = target_file.unwrap();
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
        Some(f) => match f {
            PullFormat::Dotenv => SecretsFileFormat::Dotenv,
            PullFormat::Yaml => SecretsFileFormat::Yaml,
            PullFormat::Json => SecretsFileFormat::Json,
        },
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

    let mut secrets = secrets_res.unwrap();

    if !only_set.is_empty() && !exclude_set.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        eprintln!();
        bail!(err);
    }

    if only_set.is_empty() == false {
        // filter unwanted secrets
        secrets = secrets
            .into_iter()
            .filter(|secret| only_set.contains(&secret.name))
            .collect();
    } else if exclude_set.is_empty() == false {
        secrets = secrets
            .into_iter()
            .filter(|secret| !exclude_set.contains(&secret.name))
            .collect();
    }

    if secrets.is_empty() {
        let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found");
        eprintln!("{}", msg);

        return Ok(());
    }

    if !set.is_empty() {
        let key_values_pairs = get_set_key_value_pairs(set);

        match key_values_pairs {
            Ok(set_secrets) => {
                for (name, value) in set_secrets {
                    // find index
                    let index = secrets.iter().position(|secret| secret.name == name);

                    if let Some(index) = index {
                        let existing_secret = &mut secrets[index];
                        existing_secret.value = value;
                    } else {
                        let new_secret = Secret {
                            name,
                            value,
                            description: None,
                        };

                        secrets.push(new_secret);
                    }
                }
            }
            Err(e) => {
                bail!(e);
            }
        }
    }

    let duplicate_keys = find_duplicate_keys(&secrets);

    if !duplicate_keys.is_empty() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateNames(
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

    let has_input_warning =
        !refs_validation.invalid_format.is_empty() || !refs_validation.not_found.is_empty();

    if has_input_warning {
        eprintln!();
    }

    let info = format!("Number of screts to push: {}", secrets.len());
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
