use anyhow::{bail, Result};
use colored_json::to_colored_json_auto;
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::env_changelog,
    cmd::environments::EnvChangelog,
    models::{
        api_client::GetRequestApiResponse,
        env_changelog::EnvChangelogList,
        validation::{EnvChangelogInputValidationError, InputValidationError},
    },
    utils::{spinner::request_spinner, validation::validate_project_environment},
};

pub struct HandleEnvChangelogListArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub page: Option<usize>,
    pub limit: Option<usize>,
    pub show_values: bool,
    // pub show_secrets: bool,
    // pub only_secrets: bool,
    pub raw: bool,
}

pub async fn handle_list_changelog(args: HandleEnvChangelogListArgs) -> Result<()> {
    let HandleEnvChangelogListArgs {
        api_key,
        project,
        environment,
        page,
        limit,
        show_values,
        raw,
    } = args;

    let input_valid = validate_project_environment(&project, &environment, true);

    if let Err(err) = input_valid {
        bail!(err);
    }

    if let Some(page) = page {
        if page < 1 {
            let error_msg = "argument page is invalid";
            let hint = "page must be greater than 0";

            let formatted_err = format!(
                "{}\n{}",
                "Input error".red().bold(),
                format!("- message: {error_msg}\n- hint: {hint}")
            );
            bail!(formatted_err);
        }
    }

    if let Some(limit) = limit {
        if limit < 1 || limit > 10 {
            let error =
                InputValidationError::EnvChangelog(EnvChangelogInputValidationError::InvalidLimit);

            bail!(error);
        }
    }

    if let Some(page) = page {
        if page < 1 || page > 1_000 {
            let error =
                InputValidationError::EnvChangelog(EnvChangelogInputValidationError::InvalidPage);

            bail!(error);
        }
    }

    // OK
    debug!("listing env changelog...");

    let mut spinner = request_spinner();

    let args = env_changelog::ListArgs {
        api_key,
        project,
        environment,
        show_values,
        page,
        limit,
    };

    let res = env_changelog::list(args).await;

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

            let response_data = serde_json::from_str::<EnvChangelogList>(&data.text);

            match response_data {
                Ok(list) => {
                    debug!("{:#?}", &list);

                    if raw {
                        let value = serde_json::to_value(&list).unwrap();
                        let pretty = to_colored_json_auto(&value).unwrap();
                        println!("{}", pretty);
                    } else {
                        print!("{}", list);
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
