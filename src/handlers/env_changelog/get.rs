use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;

use crate::{
    api::env_changelog,
    models::{api_client::GetRequestApiResponse, env_changelog::EnvChangelogListItem},
    utils::{
        spinner::request_spinner,
        validation::{validate_env_changelog_id, validate_project_environment},
    },
};

pub struct HandleGetEnvChangelogItemArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
    pub raw: bool,
}

pub async fn handle_get_changelog_item(args: HandleGetEnvChangelogItemArgs) -> Result<()> {
    let HandleGetEnvChangelogItemArgs {
        api_key,
        project,
        environment,
        change_id,
        raw,
    } = args;

    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    let id_validation = validate_env_changelog_id(&change_id);

    if let Err(err) = id_validation {
        bail!(err);
    }

    // OK
    debug!("gettting env changelog item...");

    let mut spinner = request_spinner();

    let args = env_changelog::GetArgs {
        api_key,
        project,
        environment,
        change_id,
    };

    let res = env_changelog::get(args).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    let res = res.unwrap();

    match res {
        GetRequestApiResponse::Ok(data) => {
            debug!("{:#?}", &data.text);
            spinner.stop_and_persist("", "");

            let response_data = serde_json::from_str::<EnvChangelogListItem>(&data.text);

            match response_data {
                Ok(item) => {
                    debug!("{:#?}", &item);

                    if raw {
                        let value = serde_json::to_value(&item).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();
                        println!("{}", pretty);
                    } else {
                        print!("{}", item);
                    }
                }
                Err(e) => {
                    debug!("Error: {:#?}", e);
                    bail!("Something went wrong")
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            debug!("Error: {:#?}", e);
            spinner.stop_with_message(&format!("{}", e));
        }
    }

    Ok(())
}
