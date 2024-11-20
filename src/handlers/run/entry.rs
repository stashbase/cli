use std::{collections::HashMap, env};

use anyhow::{bail, Context, Result};
use log::debug;
use owo_colors::OwoColorize;
use spinoff::{spinners, Color, Spinner, Streams};

use crate::{
    api::{environments, secrets},
    handlers::{pull::entry::load_from_file, run::subprocess},
    models::{
        api_client::GetRequestApiResponse,
        config_env::{ConfigActionCommand, EnvConfigItem},
        secrets::SecretWithoutDescription,
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError, RunInputValidationError,
            SecretsInputValidationError, YamlEnvConfigError,
        },
    },
    utils::{
        interaction::{self, select},
        separator,
        tables::build::build_table,
        validation::{
            validate_project_environment, validate_project_environment_identifier,
            validate_secret_names,
        },
    },
    SUBPROCESS_RUNNING,
};

use super::format::format_env_variable_value;

#[derive(Debug)]
pub struct HandleRunArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub command: Vec<String>,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub set: Vec<String>,
    pub print_secrets: bool,
    pub file: Option<String>,
    pub expand_refs: Option<bool>,
}

pub async fn handle_load_env_run(args: HandleRunArgs) -> Result<()> {
    let HandleRunArgs {
        api_key,
        command,
        file,
        mut set,
        mut project,
        mut environment,
        mut only,
        mut exclude,
        mut expand_refs,
        mut print_secrets,
    } = args;

    if file.is_some() && (project.is_some() || environment.is_some()) {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::FileArgWithInline,
        );

        bail!(err);
    }

    if command.is_empty() {
        let err = InputValidationError::Run(RunInputValidationError::NoCmdProvided);
        bail!(err);
    }

    let mut is_from_file = true;

    let mut setted_secrets = HashMap::<String, String>::new();

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
        let config_action_command = ConfigActionCommand::Run;
        // LOAD from file
        let selected_config_item = EnvConfigItem::select_from_file(file, &config_action_command)?;

        if let Some(config) = selected_config_item {
            let secrets_config = config.get_run_secrets();

            // expand refs
            if let Some(expand_refs_val) = secrets_config.expand_refs {
                if expand_refs.is_none() {
                    expand_refs = Some(expand_refs_val);
                }
            }

            // print
            if let Some(print_secrets_val) = secrets_config.print {
                print_secrets = print_secrets_val;
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
            eprintln!("\nRun command exited");
            // eprintln!("Run command exited");
            return Ok(());
        }
    }

    let project = project.unwrap();
    let environment = environment.unwrap();

    let validation_res =
        validate_project_environment_identifier(project.as_ref(), environment.as_ref(), true);

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
        let name_validation_res = validate_secret_names(&only);

        if let Err(_) = name_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::OnlyNameFormat,
            );

            if is_from_file {
                eprintln!();
            }

            bail!(err);
        }
    }

    if !exclude.is_empty() {
        let name_validation_res = validate_secret_names(&exclude);

        if let Err(_) = name_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::ExcludeNameFormat,
            );

            if is_from_file {
                eprintln!();
            }

            bail!(err);
        }
    }

    if !set.is_empty() {
        let name_values_pairs = get_set_name_value_pairs(set);

        match name_values_pairs {
            Ok(secrets) => {
                for (name, value) in secrets {
                    setted_secrets.insert(name, value);
                }
            }
            Err(e) => {
                bail!(e);
            }
        }
    }

    let setted_len = setted_secrets.len();

    if setted_len > 0 && setted_len == only.len() {
        let exists_count = setted_secrets
            .iter()
            .filter(|secret| only.contains(&secret.0))
            .count();

        if exists_count == setted_len {
            let run_error = RunInputValidationError::NoSecretsToFetch;
            let err = InputValidationError::Run(run_error);

            bail!(err);
        }
    }

    // exclude manually
    if !setted_secrets.is_empty() {
        for secret in setted_secrets.iter() {
            let name = secret.0;

            let exists = exclude.contains(&name);

            // if !exists {
            //     exclude.push(key.to_string());
            // }

            if !exists && only.is_empty() {
                exclude.push(name.to_string());
            }

            let only_exists = only.contains(&name);
            if only_exists {
                // remove from only
                let index = only.iter().position(|x| x == name).unwrap();
                only.remove(index);
            }
        }
    }

    debug!("{:#?} EXCLUDE", exclude);
    debug!("{:#?} ONLY", only);

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

    // let payload = match only.is_empty() && exclude.is_empty() && setted_secrets.is_empty() {
    //     true => None,
    //     false => {
    //         let exclude = if exclude.is_empty() && setted_secrets.is_empty() {
    //             None
    //         } else {
    //             let mut exclude_vec: Vec<String> = vec![];
    //
    //             if setted_secrets.is_empty() {
    //                 for key in &exclude {
    //                     exclude_vec.push(key.to_string());
    //                 }
    //             }
    //
    //             if !exclude.is_empty() {
    //                 for exclude_secret in exclude {
    //                     let exists = exclude_vec.contains(&exclude_secret);
    //
    //                     if !exists {
    //                         exclude_vec.push(exclude_secret.to_string());
    //                     }
    //                 }
    //             }
    //
    //             Some(exclude_vec)
    //         };
    //
    //         let only = if only.is_empty() { None } else { Some(only) };
    //
    //         let payload = LoadEnvironmentPayload { only, exclude };
    //
    //         Some(payload)
    //     }
    // };

    // let res = environments::load(
    //     api_key,
    //     project,
    //     environment,
    //     only,
    //     exclude,
    //     expand_refs.unwrap_or(false),
    // )
    // .await;

    let res = secrets::pull(
        api_key,
        project.clone(),
        environment.clone(),
        only,
        exclude,
        false,
        expand_refs.unwrap_or(false),
    )
    .await;

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

            if let Ok(mut secrets) = secrets {
                if secrets.is_empty() && setted_secrets.is_empty() {
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

                        if !setted_secrets.is_empty() {
                            for (name, value) in setted_secrets {
                                // secrets.push(SecretWithoutDescription { key, value })
                                secrets.push(SecretWithoutDescription { name, value });
                            }
                        }

                        // format secret values (remove quotes if needed)
                        for s in secrets.iter_mut() {
                            s.value = format_env_variable_value(s.value.to_string());
                        }

                        handle_run(&mut None, command, print_secrets, secrets, is_from_file)
                            .await?;
                    } else {
                        return Ok(());
                    }
                } else {
                    if !setted_secrets.is_empty() {
                        for (name, value) in setted_secrets {
                            secrets.push(SecretWithoutDescription { name, value });
                        }
                    }

                    // format secret values (remove quotes)
                    for s in secrets.iter_mut() {
                        s.value = format_env_variable_value(s.value.to_string());
                    }

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
    command: Vec<String>,
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

    debug!("{:#?}", command);
    // let mut parts = command.split_whitespace();
    // Get the first part as the command itself
    // let command = parts.next().expect("No command specified");
    // // Collect the rest as arguments
    // let arguments: Vec<&str> = parts.collect();
    // let args_strings = command
    //     .iter()
    //     .map(|s| s.to_string())
    //     .collect::<Vec<String>>();

    let env_vars = secrets;

    let mut mutex = SUBPROCESS_RUNNING.lock().unwrap();
    *mutex = true;

    let cmd = command.get(0).unwrap().to_string();

    let args = command
        .into_iter()
        .skip(1)
        .map(|s| s)
        .collect::<Vec<String>>();

    let secrets_hash_map = env_vars
        .into_iter()
        .map(|s| (s.name, s.value))
        .collect::<HashMap<String, String>>();

    // TODO: errors: no such file or directory
    subprocess::run_command(&cmd, args, secrets_hash_map)
        .await
        .unwrap_or_else(|e| {
            eprintln!("{}: {}", "Failed to run command".red(), e);
        });

    Ok(())
}

fn create_env_vars(secrets: Vec<SecretWithoutDescription>) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();

    for secret in secrets {
        map.insert(secret.name, secret.value);
    }

    map
}

fn print_table(secrets: &Vec<SecretWithoutDescription>) {
    let table = build_table(secrets);
    println!("{}\n", table);
}

pub fn get_set_name_value_pairs(values: Vec<String>) -> Result<Vec<(String, String)>> {
    let name_value_pairs_res = separator::key_value(values);

    match name_value_pairs_res {
        Ok(name_value_pairs) => {
            let names = name_value_pairs
                .iter()
                .map(|kv| format!("{}", kv.0))
                .collect::<Vec<String>>();
            // ok

            let names_validation = validate_secret_names(&names);

            match names_validation {
                Ok(_) => {
                    return Ok(name_value_pairs);
                }
                Err(err) => {
                    if let Some(validation_err) = err.downcast_ref::<InputValidationError>() {
                        let mapped_err = match validation_err {
                            InputValidationError::Secrets(
                                SecretsInputValidationError::NameFormat { multiple: _ },
                            ) => InputValidationError::LoadEnvironment(
                                LoadEnvironmentInputValidationError::SetNameValueFormat,
                            ),
                            InputValidationError::Secrets(
                                SecretsInputValidationError::NameTooShort { multiple: _ },
                            ) => InputValidationError::LoadEnvironment(
                                LoadEnvironmentInputValidationError::SetNameTooShort,
                            ),
                            InputValidationError::Secrets(
                                SecretsInputValidationError::NameTooLong { multiple: _ },
                            ) => InputValidationError::LoadEnvironment(
                                LoadEnvironmentInputValidationError::SetNameTooLong,
                            ),
                            _ => unreachable!(),
                        };

                        bail!(mapped_err);
                    } else {
                        // unreachable
                        bail!(err)
                    }
                }
            }
        }
        Err(_) => {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::SetNameValueSeparator,
            );

            bail!(err);
        }
    }
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
