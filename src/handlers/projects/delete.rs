use anyhow::{bail, Result};

use crate::{
    api::projects,
    models::api_client::DeleteRequestApiResponse,
    utils::{
        interaction, output::ColorizeIfColoredOutput, spinner::request_spinner,
        validation::validate_project_identifier,
    },
};

pub struct HandleDeleteProjectArgs {
    pub api_key: String,
    pub name: String,
    pub json_format: bool,
    pub silent: bool,
    pub force: bool,
}

pub async fn handle_delete_project(args: HandleDeleteProjectArgs) -> Result<()> {
    let HandleDeleteProjectArgs {
        api_key,
        name,
        json_format,
        silent,
        force,
    } = args;

    let identifier_is_valid = validate_project_identifier(&name, true);

    if let Err(err) = identifier_is_valid {
        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    if !force {
        eprintln!(
            "{}",
            "All environments and secrets will be deleted.".red_if_tty_stderr()
        );

        let i = interaction::input(&format!("Type '{}' to confirm.", name));

        if i != name {
            eprintln!("Input does not match, action aborted.");
            return Ok(());
        }
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let project_res = projects::delete_project(api_key, name).await;

    if let Err(err) = project_res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let project_res = project_res.unwrap();

    match project_res {
        DeleteRequestApiResponse::Ok(_) => {
            if json_format {
                if let Some(mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                println!("{{}}");
            } else {
                if let Some(mut spinner) = spinner {
                    spinner.stop_with_message("Project deleted.");
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
