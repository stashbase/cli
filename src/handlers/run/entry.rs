use std::collections::{HashMap, HashSet};

use anyhow::bail;
use log::debug;
use spinoff::{spinners, Color, Spinner, Streams};
use tabled::Tabled;

use crate::{
    api::secrets,
    handlers::run::subprocess,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        config_env::{ConfigActionCommand, EnvConfigItem},
        scope::Scope,
        secrets::{PrintSecrets, SecretOnlyName, SecretWithoutComment},
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError, RunInputValidationError,
        },
    },
    utils::{
        interaction::{self},
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
        separator,
        tables::build::build_table,
        validation::{
            map_secret_to_load_exclude_secrets_error, map_secret_to_load_only_secrets_error,
            map_secret_to_load_set_secrets_error, validate_project_environment_identifier,
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
    pub set_comments: Vec<String>,
    pub print_secrets: Option<PrintSecrets>,
    pub no_print_secrets: bool,
    pub file: Option<String>,
    pub expand_refs: Option<bool>,
    pub json_format: bool,
    pub silent: bool,
    pub scope: Option<Scope>,
}

pub async fn handle_load_env_run(args: HandleRunArgs) -> anyhow::Result<()> {
    let HandleRunArgs {
        api_key,
        command,
        file,
        mut set,
        set_comments,
        mut project,
        mut environment,
        mut only,
        mut exclude,
        mut expand_refs,
        mut print_secrets,
        no_print_secrets,
        json_format,
        silent,
        scope,
    } = args;

    if no_print_secrets {
        print_secrets = None;
    }

    // Handle environment scope - workspace scope behaves like no scope
    let is_environment_scope = scope.as_ref() == Some(&Scope::Environment);

    if !is_environment_scope && file.is_some() && (project.is_some() || environment.is_some()) {
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::FileArgWithInline,
        );
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    if command.is_empty() {
        let error = InputValidationError::Run(RunInputValidationError::NoCmdProvided);
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    if let Err(error) = validate_no_duplicate_set_names(&set) {
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    let mut is_from_file = true;

    let mut setted_secrets = HashMap::<String, String>::new();

    if is_environment_scope {
        // For environment scope, load from API (not from file)
        is_from_file = false;
    } else if let (Some(_), Some(_)) = (&project, &environment) {
        is_from_file = false;
    } else if let Some(_) = project {
        // missing env arg
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::MissingEnvArg,
        );
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    } else if let Some(_) = environment {
        // missing project error
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::MissingProjectArg,
        );
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
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
            if !no_print_secrets {
                if let Some(print_secrets_val) = secrets_config.print.clone() {
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
                    let mut seen_set_names = HashSet::new();
                    let mut duplicate_set_names = Vec::new();
                    let mut duplicate_set_seen = HashSet::new();

                    for item in set_val {
                        if !seen_set_names.insert(item.key.clone())
                            && duplicate_set_seen.insert(item.key.clone())
                        {
                            duplicate_set_names.push(item.key.clone());
                        }

                        let name_value_str = format!("{}={}", item.key, item.value);

                        if set.contains(&name_value_str) == false {
                            set_secrets_from_file.push(name_value_str);
                        }
                    }

                    if !duplicate_set_names.is_empty() {
                        let error = InputValidationError::LoadEnvironment(
                            LoadEnvironmentInputValidationError::SetDuplicateNames(
                                duplicate_set_names,
                            ),
                        );
                        let formatted_err = error.format_error_output(json_format)?;

                        if !silent {
                            eprintln!();
                        }
                        bail!(formatted_err);
                    }

                    set = [set_secrets_from_file, set].concat();
                }
            }

            project = Some(config.project);
            environment = Some(config.environment);
        } else {
            if !silent {
                eprintln!("\nRun command exited");
            }
            return Ok(());
        }
    }

    // Only validate project/environment if not using environment scope
    if !is_environment_scope {
        if let (Some(ref proj), Some(ref env)) = (&project, &environment) {
            let validation_res = validate_project_environment_identifier(proj, env, true);

            if let Err(e) = validation_res {
                let formatted_err = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(formatted_err);
            }
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
                    setted_secrets.insert(name, value);
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

    if !set_comments.is_empty() {
        let comments_pairs = get_set_name_comment_pairs(set_comments);

        match comments_pairs {
            Ok(comments) => {
                let mut missing_set_names = Vec::<String>::new();

                for (name, _) in comments {
                    if !setted_secrets.contains_key(&name) {
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

    let setted_len = setted_secrets.len();

    if setted_len > 0 && setted_len == only.len() {
        let exists_count = setted_secrets
            .iter()
            .filter(|secret| only.contains(&secret.0))
            .count();

        if exists_count == setted_len {
            let run_error = RunInputValidationError::NoSecretsToFetch;
            let error = InputValidationError::Run(run_error);
            let formatted_err = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(formatted_err);
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

    let only_len = only.len();

    if is_from_file && !silent {
        eprintln!();
    }

    let mut spinner = if !silent {
        Some(Spinner::new_with_stream(
            spinners::Dots,
            "Loading environment...",
            Color::Cyan,
            Streams::Stderr,
        ))
    } else {
        None
    };

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
        false,
        expand_refs.unwrap_or(false),
    )
    .await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        if let Some(mut spinner) = spinner {
            spinner.stop_with_message(&err.to_string());
        } else {
            eprintln!("{}", err.to_string());
        }

        return Ok(());
    }

    let res = res.unwrap();
    //
    match res {
        GetRequestApiResponse::Ok(data) => {
            // handle_ok_response(&mut spinner, command, only_len, print_secrets, data).await?;

            let secrets = serde_json::from_str::<Vec<SecretWithoutComment>>(&data.text);

            if let Ok(mut secrets) = secrets {
                if secrets.is_empty() && setted_secrets.is_empty() {
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

                        if let Some(ref mut spinner) = spinner {
                            spinner.stop_with_message(&msg);
                        } else if !silent {
                            eprintln!("{}", msg);
                        }
                    }

                    return Ok(());
                }

                if only_len > 0 && secrets.len() < only_len {
                    let mut msg = format!(
                        "{} {} Secret(s) found, {} secret(s) requested.",
                        "Error:".red_if_tty_stderr(),
                        secrets.len(),
                        only_len
                    );

                    if !is_from_file {
                        msg.insert_str(0, "\n");
                    }

                    if let Some(ref mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    if !silent {
                        eprintln!("{}", msg);
                    }

                    let confirmation = if !silent {
                        interaction::confirm_opt("Do you still want to proceed?")
                    } else {
                        Some(true) // Auto-proceed in silent mode
                    };

                    if let Some(true) = confirmation {
                        if print_secrets.is_some() {
                            eprintln!();
                        }
                        // if !print_secrets {
                        //     eprintln!();
                        // }

                        if !setted_secrets.is_empty() {
                            for (name, value) in setted_secrets {
                                // secrets.push(SecretWithoutDescription { key, value })
                                secrets.push(SecretWithoutComment { name, value });
                            }
                        }

                        // format secret values (remove quotes if needed)
                        for s in secrets.iter_mut() {
                            s.value = format_env_variable_value(s.value.to_string());
                        }

                        handle_run(
                            &mut spinner,
                            command,
                            print_secrets.clone(),
                            secrets,
                            is_from_file,
                            silent,
                            json_format,
                        )
                        .await?;
                    } else {
                        return Ok(());
                    }
                } else {
                    if !setted_secrets.is_empty() {
                        for (name, value) in setted_secrets {
                            secrets.push(SecretWithoutComment { name, value });
                        }
                    }

                    // format secret values (remove quotes)
                    for s in secrets.iter_mut() {
                        s.value = format_env_variable_value(s.value.to_string());
                    }

                    handle_run(
                        &mut spinner,
                        command,
                        print_secrets.clone(),
                        secrets,
                        is_from_file,
                        silent,
                        json_format,
                    )
                    .await?;
                }
            } else {
                if let Some(ref mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let error = OutputError::failed_to_deserialize_response_body();
                let formatted_err = error.format_error_output(json_format)?;

                bail!(formatted_err);
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(ref mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }
            bail!(e);
        }
    }
    //
    Ok(())
}

async fn handle_run(
    spinner: &mut Option<Spinner>,
    command: Vec<String>,
    print_secrets: Option<PrintSecrets>,
    secrets: Vec<SecretWithoutComment>,
    is_from_file: bool,
    silent: bool,
    json_format: bool,
) -> anyhow::Result<()> {
    if !silent {
        let mut success_msg = format!(
            "{} {} ({} {})",
            "✓".green_if_tty_stderr(),
            "Environment loaded",
            secrets.len(),
            if secrets.len() == 1 {
                "secret"
            } else {
                "secrets"
            }
        );

        if print_secrets.is_some() && !is_from_file {
            success_msg.insert_str(0, "\n");
            if let Some(spinner) = spinner {
                spinner.stop_with_message(&success_msg);
            } else {
                eprintln!("{}", success_msg);
            }
        } else {
            if let Some(spinner) = spinner {
                spinner.stop_with_message(&success_msg);
            } else {
                eprintln!("{}", success_msg);
            }
        }
    } else if let Some(spinner) = spinner {
        spinner.stop_and_persist("", "");
    }
    // success msg

    if print_secrets.is_some() && !silent {
        let print_masked = print_secrets
            .as_ref()
            .map(|p| p.is_masked())
            .unwrap_or(false);

        if print_masked {
            let formatted_secrets: Vec<SecretWithoutComment> = secrets
                .clone()
                .into_iter()
                .map(|s| SecretWithoutComment {
                    name: s.name,
                    value: if s.value.len() <= 3 {
                        "*".repeat(6)
                    } else {
                        format!("{}{}", &s.value[..3], "*".repeat(6))
                    },
                })
                .collect();

            if json_format {
                let json_str = get_formatted_json_string(&formatted_secrets, true).unwrap();
                println!("{}\n", json_str);
            } else {
                print_table(&formatted_secrets);
            }
        } else if print_secrets.is_some_and(|p| p.is_name()) {
            // print only names
            let formatted_secrets: Vec<SecretOnlyName> = secrets
                .clone()
                .into_iter()
                .map(|s| SecretOnlyName { name: s.name })
                .collect();

            if json_format {
                let json_str = get_formatted_json_string(&formatted_secrets, true).unwrap();
                println!("{}\n", json_str);
            } else {
                print_table(&formatted_secrets);
            }
        } else {
            // print full
            if json_format {
                let json_str = get_formatted_json_string(&secrets, true).unwrap();
                println!("{}\n", json_str);
            } else {
                print_table(&secrets);
            }
        }
    }

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
            eprintln!("{}: {}", "Failed to run command".red_if_tty_stderr(), e);
        });

    Ok(())
}

fn print_table(secrets: &Vec<impl Tabled>) {
    let table = build_table(secrets);
    println!("{}\n", table);
}

pub fn get_set_name_value_pairs(
    values: Vec<String>,
) -> Result<Vec<(String, String)>, InputValidationError> {
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
                    let mapped_err = map_secret_to_load_set_secrets_error(&err);
                    let error = InputValidationError::LoadEnvironment(mapped_err);

                    return Err(error);
                }
            }
        }
        Err(_) => {
            let error = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::SetSecretNameValueSeparator,
            );

            return Err(error);
        }
    }
}

pub fn validate_no_duplicate_set_names(values: &[String]) -> Result<(), InputValidationError> {
    let name_value_pairs = get_set_name_value_pairs(values.to_vec())?;
    let duplicate_names = get_duplicate_names(&name_value_pairs);

    if !duplicate_names.is_empty() {
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::SetDuplicateNames(duplicate_names),
        );
        return Err(error);
    }

    Ok(())
}

pub fn get_duplicate_names(pairs: &[(String, String)]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut duplicate_names = Vec::new();
    let mut duplicate_seen = HashSet::new();

    for (name, _) in pairs {
        if !seen.insert(name.clone()) && duplicate_seen.insert(name.clone()) {
            duplicate_names.push(name.clone());
        }
    }

    duplicate_names
}

pub fn get_set_name_comment_pairs(
    comments: Vec<String>,
) -> Result<Vec<(String, String)>, InputValidationError> {
    let name_comment_pairs_res = separator::key_value(comments);

    match name_comment_pairs_res {
        Ok(name_comment_pairs) => {
            let names = name_comment_pairs
                .iter()
                .map(|kv| format!("{}", kv.0))
                .collect::<Vec<String>>();

            let names_validation = validate_secret_names(&names);

            match names_validation {
                Ok(_) => Ok(name_comment_pairs),
                Err(err) => {
                    let mapped_err = map_secret_to_load_set_secrets_error(&err);
                    let error = InputValidationError::LoadEnvironment(mapped_err);
                    Err(error)
                }
            }
        }
        Err(_) => {
            let error = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::SetSecretNameValueSeparator,
            );

            Err(error)
        }
    }
}
