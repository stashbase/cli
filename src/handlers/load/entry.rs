use std::{collections::HashMap, env, hash::Hash};

use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    handlers::load::run::run_command,
    models::{api_client::GetRequestApiResponse, secrets::Secret},
    utils::validation::validate_project_environment,
};

#[derive(Debug)]
pub struct HandleLoadEnvironmentArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub command: String,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
}

pub async fn handle_load_environment(args: HandleLoadEnvironmentArgs) -> Result<()> {
    let HandleLoadEnvironmentArgs {
        token,
        project,
        environment,
        command,
        only,
        exclude,
    } = args;

    let validation_res = validate_project_environment(&project, &environment, true);

    if let Err(e) = validation_res {
        bail!(e);
    }

    let test_secrets: Vec<Secret> = vec![Secret {
        key: "JWT_SECRET".to_string(),
        value: "secret".to_string(),
        description: None,
    }];

    let mut parts = command.split_whitespace();
    // Get the first part as the command itself
    let command = parts.next().expect("No command specified");
    // Collect the rest as arguments
    let mut arguments: Vec<&str> = parts.collect();

    if command == "npm" {
        arguments.push("--color=always");
    }

    let env_vars = create_env_vars(test_secrets);

    run_command(command, arguments, env_vars)
        .await
        .expect("failed to run command");

    return Ok(());

    let res = secrets::list(token, project, environment, None, false).await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        panic!("{}", err);
        return Ok(());
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            let secrets = serde_json::from_str::<Vec<Secret>>(&data.text);
            match secrets {
                Ok(secrets) => {
                    debug!("{:#?}", &secrets);

                    let mut parts = command.split_whitespace();
                    // Get the first part as the command itself
                    let command = parts.next().expect("No command specified");
                    // Collect the rest as arguments
                    let arguments: Vec<&str> = parts.collect();

                    let env_vars = create_env_vars(secrets);

                    run_command(command, arguments, env_vars)
                        .await
                        .expect("failed to run command");
                }
                Err(e) => {
                    panic!("{}", e);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            panic!("{}", e);
        }
    }

    Ok(())
}

fn set_vars_from_secrets(secrets: Vec<Secret>) {
    let env_vars = create_env_vars(secrets);
    apply_env_vars(env_vars);
}

fn apply_env_vars(env_vars: HashMap<String, String>) {
    for (key, value) in env_vars {
        env::set_var(key, value);
    }
}

fn create_env_vars(secrets: Vec<Secret>) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();

    for secret in secrets {
        map.insert(secret.key, secret.value);
    }

    map
}
