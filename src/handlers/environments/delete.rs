use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::environments,
    models::api_client::DeleteRequestApiResponse,
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{validate_project_environment, validate_project_environment_identifier},
    },
};

pub async fn handle_delete_environment(
    api_key: String,
    project: String,
    environment: String,
    json_format: bool,
    silent: bool,
) -> Result<()> {
    let input_valid = validate_project_environment_identifier(&project, &environment, true);

    if let Err(err) = input_valid {
        let formatted_err = err.format_error_output(json_format)?;

        eprintln!();
        bail!(formatted_err);
    }
    // ok

    eprintln!("{}", "Environment with all secrets will be deleted.".red());

    let i = interaction::input(&format!("Type '{}' to confirm.", environment));

    if i != environment {
        println!("Input does not match, action aborted.");
        return Ok(());
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
                } else {
                    eprintln!();
                }

                if !silent {
                    println!("{{}}");
                }
            } else {
                if !silent {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_with_message("Environment deleted.");
                    } else {
                        eprintln!();
                    }
                }
            }
        }
        DeleteRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            } else {
                eprintln!();
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
