use std::{collections::HashMap, env};

use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;
use spinoff::{spinners, Color, Spinner, Streams};

use crate::{
    api::environments,
    handlers::load::run::run_command,
    models::{
        api_client::{GetApiResponseOk, GetRequestApiResponse},
        secrets::SecretWithoutDescription,
        validation::{InputValidationError, LoadEnvironmentInputValidationError},
    },
    utils::{
        tables::build::build_secrets_table,
        validation::{validate_project_environment, validate_secret_keys},
    },
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

    if !only.is_empty() && !exclude.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );
        bail!(err);
    }

    if !only.is_empty() {
        let key_validation_res = validate_secret_keys(&only);

        if let Err(_) = key_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::OnlyKeyFormat,
            );

            bail!(err);
        }
    }

    if !exclude.is_empty() {
        let key_validation_res = validate_secret_keys(&exclude);

        if let Err(_) = key_validation_res {
            let err = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::ExcludeKeyFormat,
            );

            bail!(err);
        }
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
            handle_ok_response(&mut spinner, command, print_secrets, data).await?;
        }
        GetRequestApiResponse::Err(e) => {
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}

async fn handle_ok_response(
    spinner: &mut Spinner,
    command: String,
    print_secrets: bool,
    data: GetApiResponseOk,
) -> Result<()> {
    let secrets = serde_json::from_str::<Vec<SecretWithoutDescription>>(&data.text);

    if let Ok(secrets) = secrets {
        if secrets.is_empty() {
            spinner.stop_with_message(&format!(
                "{}\n{}",
                "Error".red(),
                "- message: no secrets found"
            ));
            return Ok(());
        }

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
        if print_secrets {
            success_msg.insert_str(0, "\n");
            spinner.stop_with_message(&success_msg);
        } else {
            spinner.stop_with_message(&success_msg);
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

        let env_vars = create_env_vars(secrets);

        // TODO: errors: no such file or directory
        run_command(command, arguments, env_vars)
            .await
            .expect("failed to run command");
    } else {
        let err = secrets.unwrap_err();
        spinner.stop_with_message(&format!("{}", err));
    }

    Ok(())
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
    println!("{}\n", table);
}
