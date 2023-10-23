use std::{collections::HashMap, env};

use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;
use spinoff::{spinners, Color, Spinner, Streams};

use crate::{
    api::environments,
    handlers::load::run::run_command,
    models::{api_client::GetRequestApiResponse, secrets::SecretWithoutDescription},
    utils::{tables::build::build_secrets_table, validation::validate_project_environment},
};

#[derive(Debug)]
pub struct HandleLoadEnvironmentArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub command: String,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub print_secrets: bool,
}

pub async fn handle_load_environment(args: HandleLoadEnvironmentArgs) -> Result<()> {
    let HandleLoadEnvironmentArgs {
        token,
        project,
        environment,
        command,
        only,
        exclude,
        print_secrets,
    } = args;

    let validation_res = validate_project_environment(&project, &environment, true);

    if let Err(e) = validation_res {
        bail!(e);
    }

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

    let mut spinner = Spinner::new_with_stream(
        spinners::Dots,
        "Loading environment...",
        Color::White,
        Streams::Stderr,
    );

    let res = environments::load(token, project, environment, only, exclude).await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        spinner.stop_and_persist("", "");
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<SecretWithoutDescription>>(&data.text);

            match secrets {
                Ok(secrets) => {
                    // success msg
                    spinner.stop_with_message(&format!(
                        "{} {} ({} {})",
                        "✓".green(),
                        "Environment loaded",
                        secrets.len(),
                        if secrets.len() == 1 {
                            "secret"
                        } else {
                            "secrets"
                        }
                    ));

                    debug!("{:#?}", &secrets);

                    if print_secrets {
                        print_table(&secrets);
                    }

                    let mut parts = command.split_whitespace();
                    // Get the first part as the command itself
                    let command = parts.next().expect("No command specified");
                    // Collect the rest as arguments
                    let arguments: Vec<&str> = parts.collect();

                    let env_vars = create_env_vars(secrets);

                    // TODO: errors: no such file or directory
                    run_command(command, arguments, env_vars)
                        .await
                        .expect("failed to run command");
                }
                Err(e) => {
                    spinner.stop_with_message(&format!("{}", e));
                    // panic!("{}", e);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
            // panic!("{}", e);
        }
    }

    Ok(())
}

fn set_vars_from_secrets(secrets: Vec<SecretWithoutDescription>) {
    let env_vars = create_env_vars(secrets);
    apply_env_vars(env_vars);
}

fn apply_env_vars(env_vars: HashMap<String, String>) {
    for (key, value) in env_vars {
        env::set_var(key, value);
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
    let table = build_secrets_table(secrets);
    println!("{}", table);
}
