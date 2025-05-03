use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::UpdateSecretCommentPayload,
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        secrets::format_secret_comment,
        spinner::request_spinner,
        validation::{
            is_valid_secret_comment, validate_environment_name, validate_project_name,
            validate_secret_name,
        },
    },
};

pub struct HandleCommentArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub name: String,
    pub comment: String,
}

pub async fn handle_update_comment(args: HandleCommentArgs) -> Result<()> {
    let HandleCommentArgs {
        api_key,
        project,
        environment,
        comment,
        name,
    } = args;

    let input_validation_res = validate_input(&project, &environment, &name);

    if let Err(e) = input_validation_res {
        eprintln!();
        bail!(e);
    }

    let formatted_comment = match comment.is_empty() {
        true => "".to_string(),
        false => format_secret_comment(&comment, true),
    };

    let is_valid = is_valid_secret_comment(&formatted_comment);

    if !is_valid {
        let err = InputValidationError::Secrets(SecretsInputValidationError::CommentTooLong);

        eprintln!();
        bail!(err)
    }

    // ok
    let payload = UpdateSecretCommentPayload {
        comment: formatted_comment,
    };

    let mut spinner = request_spinner();

    let res = secrets::update_comment(api_key, project, environment, name, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(_) => {
            spinner.stop_with_message("Comment updated.");
        }
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_and_persist("", "");
            bail!(e);
        }
    }

    Ok(())
}

fn validate_input(project: &str, environment: &str, name: &str) -> Result<()> {
    let project_name_validation_res = validate_project_name(project, false, false);

    if let Err(err) = project_name_validation_res {
        bail!(err);
    }

    let env_validation_res = validate_environment_name(environment, false, false);

    if let Err(err) = env_validation_res {
        bail!(err);
    }

    let name_valid = validate_secret_name(&name);

    if let Err(err) = name_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    Ok(())
}
