use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    models::api_client::DeleteRequestApiResponse,
    utils::{
        interaction, spinner::request_spinner, validation::validate_project_environment_identifier,
    },
};

pub struct HandleDeleteEnvironmentArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

pub async fn handle_delete_environment(args: HandleDeleteEnvironmentArgs) -> Result<()> {
    let HandleDeleteEnvironmentArgs {
        api_key,
        project,
        environment,
        json_format,
        silent,
        force,
    } = args;

    let input_valid = validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_valid {
        let formatted_err = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(formatted_err);
    }

    if !force {
        eprintln!("{}", "Environment with all secrets will be deleted.".red());

        let i = interaction::input(&format!("Type '{}' to confirm.", environment));

        if i != environment {
            println!("Input does not match, action aborted.");
            return Ok(());
        }
    }

    debug!("deleting enironment...:");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = environments::delete(api_key, project, environment).await;

    if let Err(err) = res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        DeleteRequestApiResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                println!("{{}}");
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Environment deleted.");
                }
            }
        }
        DeleteRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
