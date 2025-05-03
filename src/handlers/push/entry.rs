use std::{collections::HashSet, path::Path};

use crate::{
    api::secrets,
    cmd::{pull::PullFormat, push::PushFormat, secrets::SecretsFileFormat},
    handlers::run::entry::get_set_name_value_pairs,
    models::{
        api_client::RequestApiOptionResponse,
        config_env::{ConfigActionCommand, EnvConfigItem},
        secrets::{FormatSecrets, Secret, ValidateSecrets},
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError,
            PushPullInputValidationError, SecretsInputValidationError,
        },
    },
    utils::{
        self, interaction,
        secrets::read_secrets_from_file,
        validation::{
            map_secret_to_load_exclude_secrets_error, map_secret_to_load_only_secrets_error,
            validate_project_environment_identifier, validate_secret_names,
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
    pub expand_refs: Option<bool>,
}

pub async fn handle_push(args: HandlePushArgs) -> Result<()> {
    let HandlePushArgs {
        api_key,
        config_file_path,
        only,
        exclude,
        mut expand_refs,
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

            eprintln!();
            bail!(err);
        }

        if let None = format {
            let format_config = config.get_push_format();
            format = format_config;
        }

        let secrets_config = config.get_push_secrets();

        // expand refs
        if let Some(expand_refs_val) = secrets_config.expand_refs {
            if expand_refs.is_none() {
                expand_refs = Some(expand_refs_val);
            }
        }

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

                for (name, value) in set_val {
                    let name_value_str = format!("{}={}", name, value);

                    if set.contains(&name_value_str) == false {
                        set_secrets_from_file.push(name_value_str);
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
            "file does not exist."
        );

        eprintln!();
        bail!(err_msg);
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

        eprintln!();
        bail!(err);
    }

    // validate project and environment
    let project = project.unwrap();
    let environment = environment.unwrap();

    let validation_res =
        validate_project_environment_identifier(project.as_ref(), environment.as_ref(), true);

    if let Err(e) = validation_res {
        eprintln!();
        bail!(e);
    }

    //  process, format and validate secrets
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
        let names_vec = only_set.iter().cloned().collect::<Vec<_>>();
        let name_validation_res = validate_secret_names(&names_vec);

        if let Err(err) = name_validation_res {
            if let Some(validation_err) = err.downcast_ref::<InputValidationError>() {
                let mapped_err = map_secret_to_load_only_secrets_error(&validation_err);

                eprintln!();
                bail!(InputValidationError::LoadEnvironment(mapped_err));
            }
        }

        secrets = secrets
            .into_iter()
            .filter(|secret| only_set.contains(&secret.name))
            .collect::<Vec<_>>();
    } else if !exclude_set.is_empty() {
        let names_vec = exclude_set.iter().cloned().collect::<Vec<_>>();
        let name_validation_res = validate_secret_names(&names_vec);

        if let Err(err) = name_validation_res {
            if let Some(validation_err) = err.downcast_ref::<InputValidationError>() {
                let mapped_err = map_secret_to_load_exclude_secrets_error(&validation_err);
                eprintln!();
                bail!(InputValidationError::LoadEnvironment(mapped_err));
            }
        }

        secrets = secrets
            .into_iter()
            .filter(|secret| !exclude_set.contains(&secret.name))
            .collect();
    }

    if secrets.is_empty() {
        let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found.");
        eprintln!("{}", msg);

        return Ok(());
    }

    if !set.is_empty() {
        let name_values_pairs = get_set_name_value_pairs(set);

        match name_values_pairs {
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
                            comment: None,
                        };

                        secrets.push(new_secret);
                    }
                }
            }
            Err(e) => {
                eprintln!();
                bail!(e);
            }
        }
    }

    // format secrets for input
    secrets.format();

    // validate secrets
    if let Err(err) = secrets.validate() {
        eprintln!();
        bail!(err);
    }

    let reference_warnings = secrets.get_reference_warnings();

    if !reference_warnings.is_empty() {
        eprint!("{}", reference_warnings);
    }

    let info = format!("Number of secrets to push: {}", secrets.len());
    eprintln!("{}", info);

    let confirm = interaction::confirm_opt("Are you sure you want to continue?");

    if confirm.is_none() || (confirm.unwrap() == false) {
        return Ok(());
    }
    eprintln!();

    if let Some(expand_refs) = expand_refs {
        if expand_refs == true {
            utils::secrets::expand_secret_references(&mut secrets);
        }
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
            spinner.stop_with_message("Secrets pushed.");
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    Ok(())
}
