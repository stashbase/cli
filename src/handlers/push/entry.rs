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
    pub json_format: bool,
    pub silent: bool,
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
        json_format,
        silent,
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
            let error_output = err.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
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

        if !silent {
            eprintln!();
        }
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
        let err =
            InputValidationError::Secrets(SecretsInputValidationError::ReadFile(err.to_string()));

        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    // validate project and environment
    let project = project.unwrap();
    let environment = environment.unwrap();

    let validation_res =
        validate_project_environment_identifier(project.as_ref(), environment.as_ref(), true);

    if let Err(e) = validation_res {
        let error_output = e.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    //  process, format and validate secrets
    let mut secrets = secrets_res.unwrap();

    if !only_set.is_empty() && !exclude_set.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    if only_set.is_empty() == false {
        // filter unwanted secrets
        let names_vec = only_set.iter().cloned().collect::<Vec<_>>();
        let name_validation_res = validate_secret_names(&names_vec);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_only_secrets_error(&err);
            let error_output = InputValidationError::LoadEnvironment(mapped_err)
                .format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        secrets = secrets
            .into_iter()
            .filter(|secret| only_set.contains(&secret.name))
            .collect::<Vec<_>>();
    } else if !exclude_set.is_empty() {
        let names_vec = exclude_set.iter().cloned().collect::<Vec<_>>();
        let name_validation_res = validate_secret_names(&names_vec);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_exclude_secrets_error(&err);
            let error_output = InputValidationError::LoadEnvironment(mapped_err)
                .format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(error_output);
        }

        secrets = secrets
            .into_iter()
            .filter(|secret| !exclude_set.contains(&secret.name))
            .collect();
    }

    if secrets.is_empty() {
        if !silent {
            let msg = format!("{}: {}", "Nothing to upload".yellow(), "no secrets found.");
            eprintln!("{}", msg);
        }

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
                if !silent {
                    eprintln!();
                }
                bail!(e);
            }
        }
    }

    // format secrets for input
    secrets.format();

    // validate secrets
    if let Err(err) = secrets.validate() {
        if !silent {
            eprintln!();
        }
        bail!(err);
    }

    let reference_warnings = secrets.get_reference_warnings();

    if !reference_warnings.is_empty() && !silent {
        eprint!("{}", reference_warnings);
    }

    if !silent {
        let info = format!("Number of secrets to push: {}", secrets.len());
        eprintln!("{}", info);
    }

    let confirm = if !silent {
        interaction::confirm_opt("Are you sure you want to continue?")
    } else {
        Some(true) // Auto-proceed in silent mode
    };

    if confirm.is_none() || (confirm.unwrap() == false) {
        return Ok(());
    }

    if !silent {
        eprintln!();
    }

    if let Some(expand_refs) = expand_refs {
        if expand_refs == true {
            utils::secrets::expand_secret_references(&mut secrets);
        }
    }

    let mut spinner = if !silent {
        Some(Spinner::new_with_stream(
            spinners::Dots,
            "Pushing secrets...",
            Color::Cyan,
            Streams::Stderr,
        ))
    } else {
        None
    };

    // file
    let res = secrets::set_sercrets(api_key, project, environment, &secrets).await;

    if let Err(err) = res {
        if let Some(ref mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            if json_format {
                if let Some(ref mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }
                println!("{{}}");
            } else {
                if let Some(ref mut spinner) = spinner {
                    spinner.stop_with_message("Secrets pushed.");
                } else if !silent {
                    println!("Secrets pushed.");
                }
            }
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            if let Some(ref mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
