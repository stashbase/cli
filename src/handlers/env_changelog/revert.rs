use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::env_changelog,
    models::api_client::PostPatchRequestApiResponse,
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{validate_env_changelog_id, validate_project_environment},
    },
};

pub struct HandleRevertEnvChangelogChange {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
}

pub async fn handle_revert_changelog_change(args: HandleRevertEnvChangelogChange) -> Result<()> {
    let HandleRevertEnvChangelogChange {
        token,
        project,
        environment,
        change_id,
    } = args;

    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    let id_validation = validate_env_changelog_id(&change_id);

    if let Err(err) = id_validation {
        bail!(err);
    }

    let i = interaction::confirm_opt("Are you sure?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }

    // OK
    debug!("reverting env changelog...");

    eprintln!();
    let mut spinner = request_spinner();

    let args = env_changelog::RevertArgs {
        token,
        project,
        environment,
        change_id,
    };

    let res = env_changelog::revert(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);

            // nothning to revert
            if data.status == 200 {
                spinner.stop_with_message(&format!(
                    "{}",
                    "Nothing to revert\n- details: this is current state"
                ))
            } else {
                spinner.stop_with_message(&format!(
                    "{} {}",
                    "✓".green(),
                    "Change has been reverted"
                ));
            }
        }
        PostPatchRequestApiResponse::Err(e) => {
            debug!("Error: {:#?}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
