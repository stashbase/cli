use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::Write,
};

use anyhow::{bail, Result};
use log::debug;
use spinoff::{spinners, Color, Spinner, Streams};

use crate::{
    api::secrets,
    cmd::{config::SecretsOutputFormat, pull::PullFormat},
    handlers::run::entry::{get_set_name_comment_pairs, get_set_name_value_pairs},
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        config_env::{ConfigActionCommand, EnvConfigItem},
        scope::Scope,
        secrets::{PrintSecrets, Secret, SecretOnlyName, SecretWithoutComment},
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError,
            PushPullInputValidationError, YamlEnvConfigError,
        },
    },
    utils::{
        interaction::{self, select},
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
        secrets::format_secrets,
        validation::{
            map_secret_to_load_exclude_secrets_error, map_secret_to_load_only_secrets_error,
            validate_project_environment_identifier, validate_secret_names,
        },
    },
};

#[derive(Debug)]
pub struct HandlePullArgs {
    pub api_key: String,
    pub scope: Option<Scope>,

    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub set: Vec<String>,
    pub set_comments: Vec<String>,
    pub file: Option<String>,
    pub target_file: Option<String>,
    pub format: Option<PullFormat>,
    pub expand_refs: Option<bool>,
    pub ignore_comments: Option<bool>,
    pub print_secrets: Option<PrintSecrets>,
    pub no_print_secrets: bool,
    pub overwrite_file: bool,
    pub json_format: bool,
    pub silent: bool,
}

pub async fn handle_pull(args: HandlePullArgs) -> Result<()> {
    let HandlePullArgs {
        api_key,
        scope,
        file,
        mut set,
        set_comments,
        mut target_file,
        mut format,
        mut only,
        mut exclude,
        mut expand_refs,
        mut ignore_comments,
        mut print_secrets,
        no_print_secrets,
        overwrite_file,
        json_format,
        silent,
    } = args;

    if no_print_secrets {
        print_secrets = None;
    }

    // Handle environment scope - workspace scope behaves like no scope
    let is_environment_scope = scope.as_ref() == Some(&Scope::Environment);

    let project: Option<String>;
    let environment: Option<String>;
    let mut setted_secrets = HashMap::<String, Secret>::new();
    let mut config_set_comments = HashMap::<String, String>::new();

    let config_action_command = ConfigActionCommand::Pull;

    // Handle environment scope differently - skip config file loading
    if is_environment_scope {
        // For environment scope, we don't need config file
        project = None;
        environment = None;
    } else {
        // LOAD from file
        let selected_config_item =
            EnvConfigItem::select_from_file(file.clone(), &config_action_command)?;
        if let Some(config) = selected_config_item {
            if let None = target_file {
                let target_file_path = config.get_pull_target_file();
                target_file = target_file_path;
            }

            if let None = target_file {
                let err = InputValidationError::PushPullEnvironment(
                    PushPullInputValidationError::NoFileSpecified { is_push: false },
                );

                let formatted_err = err.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }

                bail!(formatted_err);
            }

            if let None = format {
                let format_config = config.get_pull_format();
                format = format_config;
            }

            let secrets_config = config.get_pull_secrets();

            // expand refs
            if let Some(expand_refs_val) = secrets_config.expand_refs {
                if expand_refs.is_none() {
                    expand_refs = Some(expand_refs_val);
                }
            }

            // ignore comments
            if let Some(ignore_comments_val) = secrets_config.ignore_comments {
                if ignore_comments.is_none() {
                    ignore_comments = Some(ignore_comments_val);
                }
            }

            // print
            if !no_print_secrets {
                if let Some(print_secrets_val) = secrets_config.print {
                    if print_secrets.is_none() {
                        print_secrets = Some(print_secrets_val);
                    }
                }
            }

            // only
            if let Some(only_val) = secrets_config.only {
                if only_val.is_empty() == false {
                    for only_secret in only_val {
                        let already_exists = only.contains(&only_secret);

                        if !already_exists {
                            only.push(only_secret);
                        }
                    }
                }
            }

            // exclude
            if let Some(exclude_val) = secrets_config.exclude {
                if exclude_val.is_empty() == false {
                    for exclude_secret in exclude_val {
                        let already_exists = exclude.contains(&exclude_secret);

                        if !already_exists {
                            exclude.push(exclude_secret);
                        }
                    }
                }
            }

            // set
            if let Some(set_val) = secrets_config.set {
                if set_val.is_empty() == false {
                    let mut set_secrets_from_file = Vec::new();

                    for item in set_val {
                        if ignore_comments != Some(true) {
                            if let Some(comment) = item.comment {
                                config_set_comments.insert(item.key.clone(), comment);
                            }
                        }

                        let name_value_str = format!("{}={}", item.key, item.value);

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
            // eprintln!("\nRun command exited");
            // eprintln!("Run command exited");
            return Ok(());
        }
    }

    let should_ignore_comments = ignore_comments == Some(true);

    // Validation logic - skip for environment scope
    if !is_environment_scope {
        let project_ref = project.as_ref().unwrap();
        let environment_ref = environment.as_ref().unwrap();

        let validation_res =
            validate_project_environment_identifier(project_ref, environment_ref, true);

        if let Err(e) = validation_res {
            let formatted_err = e.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }

            bail!(formatted_err);
        }
    }

    if !only.is_empty() && !exclude.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        let formatted_err = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(formatted_err);
    }

    if !only.is_empty() {
        let name_validation_res = validate_secret_names(&only);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_only_secrets_error(&err);
            let error = InputValidationError::LoadEnvironment(mapped_err);
            let formatted_err = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }

            bail!(formatted_err);
        }
    }

    if !exclude.is_empty() {
        let name_validation_res = validate_secret_names(&exclude);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_exclude_secrets_error(&err);
            let error = InputValidationError::LoadEnvironment(mapped_err);
            let formatted_err = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }

            bail!(formatted_err);
        }
    }

    if !set.is_empty() {
        let name_values_pairs = get_set_name_value_pairs(set);

        match name_values_pairs {
            Ok(secrets) => {
                for (name, value) in secrets {
                    let comment = if should_ignore_comments {
                        None
                    } else {
                        config_set_comments.get(&name).cloned()
                    };
                    let secret = Secret {
                        name: name.clone(),
                        value,
                        comment,
                    };

                    setted_secrets.insert(name, secret);
                }
            }
            Err(e) => {
                let formatted_err = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }

                bail!(formatted_err);
            }
        }
    }

    if !should_ignore_comments && !set_comments.is_empty() {
        let comments_pairs = get_set_name_comment_pairs(set_comments);

        match comments_pairs {
            Ok(comments) => {
                let mut missing_set_names = Vec::<String>::new();

                for (name, comment) in comments {
                    if let Some(secret) = setted_secrets.get_mut(&name) {
                        secret.comment = Some(comment);
                    } else {
                        missing_set_names.push(name);
                    }
                }

                if !missing_set_names.is_empty() {
                    let error = InputValidationError::LoadEnvironment(
                        LoadEnvironmentInputValidationError::SetCommentWithoutSet(
                            missing_set_names,
                        ),
                    );
                    let formatted_err = error.format_error_output(json_format)?;

                    if !silent {
                        eprintln!();
                    }
                    bail!(formatted_err);
                }
            }
            Err(e) => {
                let formatted_err = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(formatted_err);
            }
        }
    }

    // exclude manually
    if !setted_secrets.is_empty() {
        for name in setted_secrets.keys() {

            let exists = exclude.contains(&name);
            if !exists {
                exclude.push(name.to_string());
            }
        }
    }

    let only_len = only.len();

    // eprintln!();

    let mut spinner = if !silent {
        Some(Spinner::new_with_stream(
            spinners::Dots,
            "Pulling environment...",
            Color::Cyan,
            Streams::Stderr,
        ))
    } else {
        None
    };

    let with_comment = match ignore_comments {
        Some(true) => false,
        Some(false) => true,
        None => true, // default to true
    };

    // Determine project and environment for API call
    let (api_project, api_environment) = if is_environment_scope {
        // For environment scope, pass None (relies on environment-scoped API key)
        (None, None)
    } else {
        (project.clone(), environment.clone())
    };

    let res = secrets::pull(
        api_key,
        api_project,
        api_environment,
        only,
        exclude,
        with_comment,
        expand_refs.unwrap_or(false),
    )
    .await;

    if let Err(err) = res {
        if let Some(ref mut spinner) = spinner {
            spinner.stop_with_message(&err.to_string());
        }
        debug!("Error: {:#?}", &err);

        let formatted_err = err.format_error_output(json_format)?;
        bail!(formatted_err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);

            match secrets {
                Ok(mut secrets) => {
                    if secrets.is_empty() && setted_secrets.is_empty() {
                        if let Some(ref mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        if json_format {
                            if only_len == 0 {
                                let message = serde_json::json!({
                                    "error": {
                                        "message": "No secrets found."
                                    }
                                });

                                let json_str = get_formatted_json_string(&message, false).unwrap();
                                eprintln!("{}", json_str);
                            } else {
                                let message = serde_json::json!({
                                    "error": {
                                        "message": format!("{} secret(s) requested, no secrets found.", only_len)
                                    }
                                });

                                let json_str = get_formatted_json_string(&message, false).unwrap();
                                eprintln!("{}", json_str);
                            }
                        } else {
                            let msg = if only_len == 0 {
                                format!(
                                    "{}\n{}",
                                    "Error".red_if_tty_stderr(),
                                    "  Message: No secrets found."
                                )
                            } else {
                                format!(
                                    "{}\n{} ({} requested)",
                                    "Error".red_if_tty_stderr(),
                                    "  Message: No secrets found.",
                                    only_len
                                )
                            };
                            eprintln!("{}", msg);
                        }

                        return Ok(());
                    }

                    if only_len > 0 && secrets.len() < only_len {
                        if let Some(ref mut spinner) = spinner {
                            spinner.stop_and_persist("", "");
                        }

                        if !silent {
                            let mut msg = format!(
                                "{} {} Secret(s) found, {} secret(s) requested.",
                                "Error:".red_if_tty_stderr(),
                                secrets.len(),
                                only_len
                            );

                            msg.insert_str(0, "\n");
                            eprintln!("{}", msg);
                        }

                        let confirmation = if !silent {
                            interaction::confirm_opt("Do you still want to proceed?")
                        } else {
                            Some(true) // Auto-proceed in silent mode
                        };

                        if let Some(true) = confirmation {
                            if !setted_secrets.is_empty() {
                                for (_, secret) in setted_secrets {
                                    secrets.push(secret);
                                }
                            }

                            // save file

                            let output_path = target_file.clone().unwrap();

                            if fs::metadata(&output_path).is_ok() && overwrite_file != true {
                                if !silent {
                                    eprintln!(
                                        "{}",
                                        &format!("File '{}' already exists.", output_path)
                                    );
                                }

                                let confirmation = if !silent {
                                    interaction::confirm_opt("Do you want to overwrite the file?")
                                } else {
                                    Some(true) // Auto-proceed in silent mode
                                };

                                if let Some(true) = confirmation {
                                    // continue
                                } else {
                                    return Ok(());
                                }
                            }

                            if let None = format {
                                if output_path.ends_with(".yaml") || output_path.ends_with(".yml") {
                                    format = Some(PullFormat::Yaml)
                                } else if output_path.ends_with(".json") {
                                    format = Some(PullFormat::Json)
                                } else {
                                    format = Some(PullFormat::Dotenv)
                                }
                            }

                            let file_string = match format {
                                Some(f) => match f {
                                    PullFormat::Json => {
                                        serde_json::to_string_pretty(&secrets).unwrap()
                                    }
                                    _ => {
                                        // yaml or dotenv
                                        let secrets_format =
                                            SecretsOutputFormat::try_from(f).unwrap();

                                        let str = format_secrets(secrets.clone(), &secrets_format);
                                        let prefix = if is_environment_scope {
                                            format!("")
                                        } else {
                                            format!(
                                                "## ------\n## Project: {}\n## Environment: {}\n## ------\n\n",
                                                project.as_ref().unwrap(), environment.as_ref().unwrap(),
                                            )
                                        };

                                        prefix + &str
                                    }
                                },
                                None => unreachable!(),
                            };

                            let file_res = write_file(&output_path, file_string);

                            match file_res {
                                Ok(_) => {
                                    if !silent {
                                        if let Some(ps) = &print_secrets {
                                            print_secrets_output(&secrets, ps, json_format);
                                        }
                                    }

                                    if !silent {
                                        if json_format {
                                            let message = serde_json::json!({
                                                "message": format!("File '{}' successfully created.", output_path)
                                            });

                                            let json_str =
                                                get_formatted_json_string(&message, false).unwrap();
                                            println!("{}", json_str);
                                        } else {
                                            println!(
                                                "{}",
                                                &format!(
                                                    "File '{}' successfully created.",
                                                    output_path
                                                )
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    bail!(e)
                                }
                            }
                        } else {
                            return Ok(());
                        }
                    } else {
                        if !setted_secrets.is_empty() {
                            for (_, secret) in setted_secrets {
                                secrets.push(secret);
                            }
                        }

                        let output_path = target_file.clone().unwrap();
                        let file_exists = fs::metadata(&output_path).is_ok();

                        if file_exists && overwrite_file != true {
                            if let Some(ref mut spinner) = spinner {
                                spinner.stop_with_message(&format!(
                                    "File '{}' already exists.",
                                    output_path
                                ));
                            }

                            let confirmation = if !silent {
                                interaction::confirm_opt("Do you want to overwrite the file?")
                            } else {
                                Some(true) // Auto-proceed in silent mode
                            };

                            if let Some(true) = confirmation {
                                // continue
                            } else {
                                return Ok(());
                            }
                        }

                        if let None = format {
                            if output_path.ends_with(".yaml") || output_path.ends_with(".yml") {
                                format = Some(PullFormat::Yaml)
                            } else if output_path.ends_with(".json") {
                                format = Some(PullFormat::Json)
                            } else {
                                format = Some(PullFormat::Dotenv)
                            }
                        }

                        let file_string = match format {
                            Some(f) => match f {
                                PullFormat::Json => serde_json::to_string_pretty(&secrets).unwrap(),
                                _ => {
                                    // yaml or dotenv
                                    let secrets_format = SecretsOutputFormat::try_from(f).unwrap();

                                    let str = format_secrets(secrets.clone(), &secrets_format);
                                    let prefix = if is_environment_scope {
                                        "## ------\n## Environment Scope\n## ------\n\n".to_string()
                                    } else {
                                        format!(
                                            "## ------\n## Project: {}\n## Environment: {}\n## ------\n\n",
                                            project.as_ref().unwrap(), environment.as_ref().unwrap(),
                                        )
                                    };

                                    prefix + &str
                                }
                            },
                            None => unreachable!(),
                        };

                        let file_res = write_file(&output_path, file_string);

                        match file_res {
                            Ok(_) => {
                                if !silent {
                                    if let Some(ps) = &print_secrets {
                                        print_secrets_output(&secrets, ps, json_format);
                                    }
                                }

                                if !file_exists {
                                    if json_format {
                                        if let Some(ref mut spinner) = spinner {
                                            spinner.stop_and_persist("", "");
                                        }

                                        let message = serde_json::json!({
                                            "message": format!("File '{}' successfully created.", output_path)
                                        });

                                        let json_str =
                                            get_formatted_json_string(&message, false).unwrap();
                                        println!("{}", json_str);
                                    } else {
                                        if let Some(ref mut spinner) = spinner {
                                            spinner.stop_with_message(&format!(
                                                "File '{}' successfully created.",
                                                output_path
                                            ));
                                        }
                                    }
                                } else {
                                    if !silent {
                                        if json_format {
                                            let message = serde_json::json!({
                                                "message": format!("File '{}' successfully created.", output_path)
                                            });

                                            let json_str =
                                                get_formatted_json_string(&message, false).unwrap();
                                            println!("{}", json_str);
                                        } else {
                                            println!(
                                                "{}",
                                                &format!(
                                                    "File '{}' successfully created.",
                                                    output_path
                                                )
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                if !file_exists {
                                    if let Some(ref mut spinner) = spinner {
                                        spinner.stop_and_persist("", "");
                                    }
                                }
                                bail!(e)
                            }
                        }
                    }
                }
                Err(_) => {
                    if let Some(ref mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(ref mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let formatted_err = e.format_error_output(json_format)?;
            bail!(formatted_err);
        }
    }

    Ok(())
}

fn write_file(file_path: &str, file_string: String) -> Result<()> {
    // Open or create a file for writing
    let mut file = match File::create(&file_path) {
        Ok(file) => file,
        Err(e) => bail!(e),
    };

    // The content you want to write to the file

    // Write the content to the file
    match file.write_all(file_string.as_bytes()) {
        Ok(_) => Ok(()),
        Err(e) => bail!(e),
    }
}

fn print_secrets_output(secrets: &[Secret], print_secrets: &PrintSecrets, json_format: bool) {
    if print_secrets.is_masked() {
        let masked_secrets: Vec<SecretWithoutComment> = secrets
            .iter()
            .map(|secret| SecretWithoutComment {
                name: secret.name.clone(),
                value: if secret.value.len() <= 3 {
                    "*".repeat(6)
                } else {
                    format!("{}{}", &secret.value[..3], "*".repeat(6))
                },
            })
            .collect();

        if json_format {
            if let Ok(json_str) = get_formatted_json_string(&masked_secrets, true) {
                println!("{}\n", json_str);
            }
        } else {
            let table = crate::utils::tables::build::build_table(&masked_secrets);
            println!("{}\n", table);
        }
    } else if print_secrets.is_name() {
        let names: Vec<SecretOnlyName> = secrets
            .iter()
            .map(|secret| SecretOnlyName {
                name: secret.name.clone(),
            })
            .collect();

        if json_format {
            if let Ok(json_str) = get_formatted_json_string(&names, true) {
                println!("{}\n", json_str);
            }
        } else {
            let table = crate::utils::tables::build::build_table(&names);
            println!("{}\n", table);
        }
    } else {
        let full_secrets: Vec<SecretWithoutComment> = secrets
            .iter()
            .map(|secret| SecretWithoutComment {
                name: secret.name.clone(),
                value: secret.value.clone(),
            })
            .collect();

        if json_format {
            if let Ok(json_str) = get_formatted_json_string(&full_secrets, true) {
                println!("{}\n", json_str);
            }
        } else {
            let table = crate::utils::tables::build::build_table(&full_secrets);
            println!("{}\n", table);
        }
    }
}

#[allow(dead_code)]
pub fn load_from_file(
    relative_path: Option<String>,
    config_action_command: &ConfigActionCommand,
) -> Result<Option<EnvConfigItem>> {
    // Load from file
    let file_path = match &relative_path {
        Some(relative_path) => {
            let mut path = std::env::current_dir()?;
            path.push(relative_path);
            path
        }
        None => env::current_dir()?.join("stashbase.yaml"),
    };
    let file_exists = file_path.exists();

    if !file_exists {
        let file_not_found_error = YamlEnvConfigError::FileNotFound {
            custom_path: if relative_path.is_some() { true } else { false },
        };

        let err = InputValidationError::YamlConfigFile(file_not_found_error);
        bail!(err);
    } else {
        let file_content_res = std::fs::read_to_string(file_path);

        if let Err(e) = file_content_res {
            let failed_to_read_err = YamlEnvConfigError::FailedToRead {
                custom_path: if relative_path.is_some() { true } else { false },
                message: e.to_string(),
            };

            let err = InputValidationError::YamlConfigFile(failed_to_read_err);
            bail!(err);
        }

        let file_content = file_content_res.unwrap();
        let deserialized_config_res = serde_yaml::from_str::<Vec<EnvConfigItem>>(&file_content);

        if let Err(e) = deserialized_config_res {
            let failed_to_read_err = YamlEnvConfigError::FailedToRead {
                custom_path: if relative_path.is_some() { true } else { false },
                message: e.to_string(),
            };

            let err = InputValidationError::YamlConfigFile(failed_to_read_err);
            bail!(err);
        }

        let deserialized_config = deserialized_config_res.unwrap();
        let len = deserialized_config.len();

        if len == 0 {
            let err = InputValidationError::YamlConfigFile(YamlEnvConfigError::NoEntries);
            bail!(err);
        } else {
            if len == 1 {
                let item = deserialized_config[0].clone();
                return Ok(Some(item));
            } else {
                let items = deserialized_config
                    .iter()
                    .map(|item| item.get_print_string(config_action_command))
                    .collect();
                // select project
                let selection = select("Select environment config", items);

                if let Some(selection) = selection {
                    let item = deserialized_config[selection].clone();

                    return Ok(Some(item));
                } else {
                    return Ok(None);
                }
            }
        }
    }
}
