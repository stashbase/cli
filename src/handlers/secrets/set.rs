use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{api_client::PostPatchRequestApiResponse, secrets::Secret},
    utils::{separator, spinner::request_spinner, validation::validate_secret_keys},
};

pub struct HandleSetSecretsArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub values: Vec<String>,
    pub description: Vec<String>,
}

// NOTE: for now must have at least one value -> validate length
// TODO: validation
pub async fn handle_set_secrets(args: HandleSetSecretsArgs) -> Result<()> {
    let HandleSetSecretsArgs {
        token,
        project,
        environment,
        values,
        description,
    } = args;

    debug!("{:#?}", description);
    // TODO: validation

    let key_value_pairs = separator::key_value(values);

    debug!("{:#?}", key_value_pairs);

    if let Err(err) = key_value_pairs {
        // TODO: error
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let key_value_pairs = key_value_pairs.unwrap();

    // validate keys
    let keys: Vec<_> = key_value_pairs
        .iter()
        .map(|kv| format!("{}", kv.0))
        .collect();

    let keys_valid = validate_secret_keys(&keys);

    if let Err(err) = keys_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let description_pairs = separator::key_value(description);
    debug!("{:#?}", description_pairs);

    if let Err(err) = description_pairs {
        // TODO: error
        bail!("{} {}", format!("Input error:").red(), err);
    }

    let description_pairs = description_pairs.unwrap();

    let payload = key_value_pairs
        .into_iter()
        .map(|x| {
            let description = description_pairs.iter().find(|d| d.0 == x.0);

            match description {
                Some((_, d_value)) => Secret {
                    key: x.0,
                    value: x.1,
                    description: Some(d_value.to_string()),
                },
                None => Secret {
                    key: x.0,
                    value: x.1,
                    description: None,
                },
            }
        })
        .collect::<_>();

    let mut spinner = request_spinner();
    let res = secrets::set_sercrets(token, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        PostPatchRequestApiResponse::Ok(_) => {
            spinner.stop_with_message(&format!("{} {}", "✓".green(), "Secrets have been setted!"));
        }
        PostPatchRequestApiResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_with_message(&format!("\n{}", e));
        }
    }

    Ok(())
}
