use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    io::Write,
    ptr::write_bytes,
};

use anyhow::{bail, Context, Result};
use log::debug;
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use spinoff::{spinners, Color, Spinner, Streams};

use crate::{
    api::{environments, secrets},
    cmd::{pull::PullFormat, secrets::SecretsFromat},
    handlers::run::entry::{get_set_key_value_pairs, load_from_file},
    models::{
        api_client::GetRequestApiResponse,
        config_env::{EnvConfigItem, EnvConfigItemSecrets},
        secrets::Secret,
        validation::{InputValidationError, LoadEnvironmentInputValidationError},
    },
    utils::{
        interaction::{self, select},
        secrets::format_secrets,
        validation::{validate_project_environment, validate_secret_keys},
    },
};

#[derive(Debug)]
pub struct HandlePullArgs {
    pub token: String,

    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub set: Vec<String>,
    pub print_secrets: bool,
    pub file: Option<String>,
    pub output_file: Option<String>,
    pub format: Option<PullFormat>,
}

pub async fn handle_pull(args: HandlePullArgs) -> Result<()> {
    let HandlePullArgs {
        token,
        file,
        set,
        mut output_file,
        mut format,
        mut only,
        mut exclude,
        mut print_secrets,
    } = args;

    let mut project: Option<String> = None;
    let mut environment: Option<String> = None;
    let mut setted_secrets = HashMap::<String, String>::new();

    // LOAD from file
    let file_config = load_from_file(file.clone())?;
    debug!("file_config: {:?}", file_config);

    if let Some(config) = file_config {
        debug!("config: {:?}", config);

        project = Some(config.project);
        environment = Some(config.environment);

        if let Some(pull_config) = config.pull {
            // check format
            if let None = format {
                format = pull_config.format;
            }

            if let None = output_file {
                output_file = Some(pull_config.file);
            }
        } else {
            if output_file.is_none() {
                bail!("No pull config for selected environment and no output argument");
            }
        }

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

            // manually set
            if let Some(set_val) = secrets.set {
                if set_val.is_empty() == false {
                    setted_secrets = set_val;
                }
            }
        }
    } else {
        // eprintln!("\nRun command exited");
        // eprintln!("Run command exited");
        return Ok(());
    }

    let project = project.unwrap();
    let environment = environment.unwrap();

    let validation_res = validate_project_environment(project.as_ref(), environment.as_ref(), true);

    if let Err(e) = validation_res {
        bail!(e);
    }

    if !only.is_empty() && !exclude.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        eprintln!();
        bail!(err);
    }

    if !only.is_empty() {
        let key_validation_res = validate_secret_keys(&only);

        if let Err(_) = key_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::OnlyKeyFormat,
            );

            eprintln!();
            bail!(err);
        }
    }

    if !exclude.is_empty() {
        let key_validation_res = validate_secret_keys(&exclude);

        if let Err(_) = key_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::ExcludeKeyFormat,
            );

            eprintln!();
            bail!(err);
        }
    }

    if !set.is_empty() {
        let key_values_pairs = get_set_key_value_pairs(set);

        match key_values_pairs {
            Ok(secrets) => {
                for (key, value) in secrets {
                    setted_secrets.insert(key, value);
                }
            }
            Err(e) => {
                bail!(e);
            }
        }
    }

    // exclude manually
    if !setted_secrets.is_empty() {
        for secret in setted_secrets.iter() {
            let key = secret.0;

            let exists = exclude.contains(&key);
            if !exists {
                exclude.push(key.to_string());
            }
        }
    }

    debug!("{:#?}", exclude);
    let only_len = only.len();

    eprintln!();

    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Pulling environment...",
        Color::Cyan,
        Streams::Stderr,
    );

    // TODO: get with descriptions
    // list secrets, not load

    let res = secrets::pull(token, project.clone(), environment.clone(), only, exclude).await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        spinner.stop_with_message(&err.to_string());

        return Ok(());
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);

            match secrets {
                Ok(mut secrets) => {
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

                        msg.insert_str(0, "\n");

                        spinner.stop_with_message(&msg);

                        let confirmation =
                            interaction::confirm_opt("Do you still want to proceed?");

                        if let Some(true) = confirmation {
                            if print_secrets {
                                eprintln!();
                            }

                            if !setted_secrets.is_empty() {
                                for (key, value) in setted_secrets {
                                    let secret = Secret {
                                        key,
                                        value,
                                        description: None,
                                    };

                                    secrets.push(secret);
                                }
                            }

                            // save file

                            let output_path = output_file.clone().unwrap();

                            if fs::metadata(&output_path).is_ok() {
                                spinner.stop_with_message(&format!(
                                    "File '{}' already exists",
                                    output_path
                                ));

                                let confirmation =
                                    interaction::confirm_opt("Do you want to overwrite the file?");

                                if let Some(true) = confirmation {
                                    // continue
                                } else {
                                    return Ok(());
                                }
                            }

                            let file_string = match format {
                                Some(f) => match f {
                                    PullFormat::Dotenv => {
                                        let str = format_secrets(secrets, &SecretsFromat::Dotenv);
                                        let prefix = format!(
                                        "## ------\n## Project:{}\n## Environment: {}\n## ------\n\n",
                                        project, environment,
                                    );

                                        prefix + &str
                                    }
                                    PullFormat::Json => {
                                        serde_json::to_string_pretty(&secrets).unwrap()
                                    }
                                },
                                None => {
                                    let str = format_secrets(secrets, &SecretsFromat::Dotenv);
                                    let prefix = format!(
                                        "## ------\n## Project:{}\n## Environment: {}\n## ------\n\n",
                                        project, environment,
                                    );

                                    prefix + &str
                                }
                            };

                            let file_res = write_file(&output_path, file_string);

                            match file_res {
                                Ok(_) => println!(
                                    "{}",
                                    &format!("File '{}' successfully created", output_path)
                                ),
                                Err(e) => {
                                    bail!(e)
                                }
                            }
                        } else {
                            return Ok(());
                        }
                    } else {
                        if !setted_secrets.is_empty() {
                            for (key, value) in setted_secrets {
                                let secret = Secret {
                                    key,
                                    value,
                                    description: None,
                                };

                                secrets.push(secret);
                            }
                        }

                        let output_path = output_file.clone().unwrap();
                        let file_exists = fs::metadata(&output_path).is_ok();

                        if file_exists {
                            spinner.stop_with_message(&format!(
                                "File '{}' already exists",
                                output_path
                            ));
                            let confirmation =
                                interaction::confirm_opt("Do you want to overwrite the file?");

                            if let Some(true) = confirmation {
                                // continue
                            } else {
                                return Ok(());
                            }
                        }

                        let file_string = match format {
                            Some(f) => match f {
                                PullFormat::Dotenv => {
                                    let str = format_secrets(secrets, &SecretsFromat::Dotenv);
                                    let prefix = format!(
                                        "## ------\n## Project:{}\n## Environment: {}\n## ------\n\n",
                                        project, environment,
                                    );

                                    prefix + &str
                                }
                                PullFormat::Json => serde_json::to_string_pretty(&secrets).unwrap(),
                            },
                            None => {
                                let str = format_secrets(secrets, &SecretsFromat::Dotenv);
                                let prefix = format!(
                                    "## ------\n## Project:{}\n## Environment: {}\n## ------\n\n",
                                    project, environment,
                                );

                                prefix + &str
                            }
                        };

                        let file_res = write_file(&output_path, file_string);

                        match file_res {
                            Ok(_) => {
                                if !file_exists {
                                    spinner.stop_with_message(&format!(
                                        "File '{}' successfully created",
                                        output_path
                                    ));
                                } else {
                                    println!(
                                        "{}",
                                        &format!("File '{}' successfully created", output_path)
                                    );
                                }
                            }
                            Err(e) => {
                                if !file_exists {
                                    spinner.stop_and_persist("", "");
                                }
                                bail!(e)
                            }
                        }
                    }
                }
                Err(_) => {
                    spinner.stop_and_persist("", "");
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_and_persist("", "");
            bail!("{}", e);
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
