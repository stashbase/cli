use std::{collections::HashMap, env};

use anyhow::{bail, Context, Result};
use log::debug;
use owo_colors::OwoColorize;
use spinoff::{spinners, Color, Spinner, Streams};

use crate::{
    api::environments,
    handlers::run::subprocess,
    models::{
        api_client::GetRequestApiResponse,
        config_env::EnvConfigItem,
        secrets::SecretWithoutDescription,
        validation::{InputValidationError, LoadEnvironmentInputValidationError},
    },
    utils::{
        interaction::{self, select},
        tables::build::build_table,
        validation::{validate_project_environment, validate_secret_keys},
    },
};

#[derive(Debug)]
pub struct HandleRunArgs {
    pub token: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub command: String,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub print_secrets: bool,
    pub file: Option<String>,
}

pub async fn handle_load_env_run(args: HandleRunArgs) -> Result<()> {
    let HandleRunArgs {
        token,
        command,
        file,
        mut project,
        mut environment,
        mut only,
        mut exclude,
        mut print_secrets,
    } = args;

    if file.is_some() && (project.is_some() || environment.is_some()) {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::FileArgWithInline,
        );

        bail!(err);
    }

    let mut is_from_file = true;

    if let (Some(_), Some(_)) = (&project, &environment) {
        is_from_file = false;
    } else if let Some(_) = project {
        // missing env arg
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::MissingEnvArg,
        );
        bail!(err);
    } else if let Some(_) = environment {
        // missing project error
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::MissingProjectArg,
        );
        bail!(err);
    } else {
        // LOAD from file
        let file_config = load_from_file(file)?;

        if let Some(config) = file_config {
            project = Some(config.project);
            environment = Some(config.environment);

            if let Some(secrets) = config.secrets {
                // print
                if let Some(print_secrets_val) = secrets.print {
                    print_secrets = print_secrets_val;
                }

                // only
                if let Some(only_val) = secrets.only {
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
                if let Some(exclude_val) = secrets.exclude {
                    if exclude_val.is_empty() == false {
                        for exclude_secret in exclude_val {
                            let already_exists = exclude.contains(&exclude_secret);

                            if !already_exists {
                                exclude.push(exclude_secret);
                            }
                        }
                    }
                }
            }
        } else {
            eprintln!("\nRun command exited");
            // eprintln!("Run command exited");
            return Ok(());
        }
    }

    let project = project.unwrap();
    let environment = environment.unwrap();

    let validation_res = validate_project_environment(project.as_ref(), environment.as_ref(), true);

    if let Err(e) = validation_res {
        if is_from_file {
            // eprintln!();
        }
        bail!(e);
    }

    if !only.is_empty() && !exclude.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        if is_from_file {
            eprintln!();
        }

        bail!(err);
    }

    if !only.is_empty() {
        let key_validation_res = validate_secret_keys(&only);

        if let Err(_) = key_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::OnlyKeyFormat,
            );

            if is_from_file {
                eprintln!();
            }

            bail!(err);
        }
    }

    if !exclude.is_empty() {
        let key_validation_res = validate_secret_keys(&exclude);

        if let Err(_) = key_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::ExcludeKeyFormat,
            );

            if is_from_file {
                eprintln!();
            }

            bail!(err);
        }
    }

    let only_len = only.len();

    if is_from_file {
        eprintln!();
    }

    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Loading environment...",
        Color::Cyan,
        Streams::Stderr,
    );

    let res = environments::load(token, project, environment, only, exclude).await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        spinner.stop_with_message(&err.to_string());

        return Ok(());
    }

    let res = res.unwrap();
    //
    match res {
        GetRequestApiResponse::Ok(data) => {
            // handle_ok_response(&mut spinner, command, only_len, print_secrets, data).await?;

            let secrets = serde_json::from_str::<Vec<SecretWithoutDescription>>(&data.text);

            if let Ok(secrets) = secrets {
                if secrets.is_empty() {
                    let msg = if only_len == 0 {
                        format!("{}\n{}", "Error".red(), "- message: no secrets found")
                    } else {
                        format!(
                            "{}\n{} ({} requested)",
                            "Error".red(),
                            "- message: no secrets found",
                            only_len
                        )
                    };

                    spinner.stop_with_message(&msg);
                    return Ok(());
                }

                if only_len > 0 && secrets.len() < only_len {
                    let mut msg = format!(
                        "{} {} secret(s) found, {} secret(s) requested",
                        "Error:".red(),
                        secrets.len(),
                        only_len
                    );

                    if !is_from_file {
                        msg.insert_str(0, "\n");
                    }

                    spinner.stop_with_message(&msg);

                    let confirmation = interaction::confirm_opt("Do you still want to proceed?");

                    if let Some(true) = confirmation {
                        if print_secrets {
                            eprintln!();
                        }
                        // if !print_secrets {
                        //     eprintln!();
                        // }

                        handle_run(&mut None, command, print_secrets, secrets, is_from_file)
                            .await?;
                    } else {
                        return Ok(());
                    }
                } else {
                    handle_run(
                        &mut Some(spinner),
                        command,
                        print_secrets,
                        secrets,
                        is_from_file,
                    )
                    .await?;
                }
            } else {
                let err = secrets.unwrap_err();
                spinner.stop_with_message(&format!("{}", err));
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }
    //
    Ok(())
}

async fn handle_run(
    spinner: &mut Option<Spinner>,
    command: String,
    print_secrets: bool,
    secrets: Vec<SecretWithoutDescription>,
    is_from_file: bool,
) -> Result<()> {
    let mut success_msg = format!(
        "{} {} ({} {})",
        "✓".green(),
        "Environment loaded",
        secrets.len(),
        if secrets.len() == 1 {
            "secret"
        } else {
            "secrets"
        }
    );

    if print_secrets && !is_from_file {
        success_msg.insert_str(0, "\n");
        if let Some(spinner) = spinner {
            spinner.stop_with_message(&success_msg);
        } else {
            println!("{}", success_msg);
        }
    } else {
        if let Some(spinner) = spinner {
            spinner.stop_with_message(&success_msg);
        } else {
            println!("{}", success_msg);
        }
    }
    // success msg

    debug!("{:#?}", &secrets);

    if print_secrets {
        print_table(&secrets);
    }

    let mut parts = command.split_whitespace();
    // Get the first part as the command itself
    let command = parts.next().expect("No command specified");
    // Collect the rest as arguments
    let arguments: Vec<&str> = parts.collect();
    let args_strings = arguments
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    let env_vars = create_env_vars(secrets);

    // TODO: errors: no such file or directory
    subprocess::run_command(command, args_strings, env_vars)
        .await
        .expect("failed to run command");

    Ok(())
}

fn load_from_file(relative_path: Option<String>) -> Result<Option<EnvConfigItem>> {
    // Load from file
    let file_path = match &relative_path {
        Some(relative_path) => {
            let mut path = std::env::current_dir()?;
            path.push(relative_path);
            path
        }
        None => env::current_dir()?.join("env-ease.yaml"),
    };
    let file_exists = file_path.exists();

    if !file_exists {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::NoConfigFile {
                custom_path: if relative_path.is_some() { true } else { false },
            },
        );

        bail!(err);
    } else {
        let file_content = std::fs::read_to_string(file_path)?;
        let deserialized_config = serde_yaml::from_str::<Vec<EnvConfigItem>>(&file_content)
            .context(format!("{}", "Failed to read env config file".red()))?;

        debug!("deserialized_config: {:?}", deserialized_config);

        let len = deserialized_config.len();

        if len == 0 {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::NoConfigFileEntries,
            );

            bail!(err);
        } else {
            if len == 1 {
                let item = deserialized_config[0].clone();
                return Ok(Some(item));
            } else {
                let items = deserialized_config
                    .iter()
                    .map(|item| item.to_string())
                    .collect();
                // select project
                let selection = select("Select environment config", items);

                debug!("selection: {:?}", selection);

                if let Some(selection) = selection {
                    let item = deserialized_config[selection].clone();
                    debug!("item: {:?}", item);

                    return Ok(Some(item));
                } else {
                    return Ok(None);
                }
            }
        }
    }
}

fn create_env_vars(secrets: Vec<SecretWithoutDescription>) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();

    for secret in secrets {
        map.insert(secret.key, secret.value);
    }

    map
}

fn print_table(secrets: &Vec<SecretWithoutDescription>) {
    let table = build_table(secrets);
    println!("{}\n", table);
}

fn test() {
    // let test_secrets: Vec<Secret> = vec![Secret {
    //     key: "JWT_SECRET".to_string(),
    //     value: "secret".to_string(),
    //     description: None,
    // }];
    //
    // let mut parts = command.split_whitespace();
    // // Get the first part as the command itself
    // let command = parts.next().expect("No command specified");
    // // Collect the rest as arguments
    // let mut arguments: Vec<&str> = parts.collect();
    //
    // if command == "npm" {
    //     arguments.push("--color=always");
    // }
    //
    // let env_vars = create_env_vars(test_secrets);
    //
    // run_command(command, arguments, env_vars)
    //     .await
    //     .expect("failed to run command");
    //
    // return Ok(());
}
