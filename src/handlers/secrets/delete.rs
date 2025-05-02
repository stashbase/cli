use anyhow::{bail, Result};
use log::{debug, error};
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::{DeleteRequestApiResponse, RequestApiOptionResponse},
        secrets::{DeleteAllSecretsResponse, DeleteSecretsResponse},
    },
    utils::{
        interaction,
        spinner::request_spinner,
        validation::{validate_environment_name, validate_project_name, validate_secret_names},
    },
};

pub struct HandleDeleteSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub names: Vec<String>,
    pub delete_all: bool,
}

// ✓
pub async fn handle_delete_secrets(args: HandleDeleteSecretsArgs) -> Result<()> {
    let HandleDeleteSecretsArgs {
        api_key,
        project,
        environment,
        delete_all,
        names,
    } = args;

    if names.is_empty() && !delete_all {
        let msg = format!(
            "{} {}",
            "Input error:".red(),
            "No secrets to delete provided."
        );
        bail!("{}", msg);
    }

    let validation_res = validate_input(&project, &environment, &names);

    if let Err(e) = validation_res {
        bail!("{}", e);
    }

    // op
    if delete_all {
        eprintln!(
            "{}",
            "All secrets in selected environment will be deleted.".red()
        );
    }

    let i = interaction::confirm_opt("Are you sure you want to continue?");

    if i.is_none() || (i.unwrap() == false) {
        return Ok(());
    }
    debug!("deleting secrets...:");

    let mut spinner = request_spinner();

    match delete_all {
        true => {
            let res = secrets::delete_all(api_key, project, environment).await;

            if let Err(err) = res {
                spinner.stop_and_persist("", "");
                error!("{:#?}", &err);
                bail!(err);
            }

            let res = res.unwrap();

            match res {
                RequestApiOptionResponse::Ok(res) => {
                    match res.text {
                        Some(text) => {
                            //
                            let json_data = serde_json::from_str::<DeleteAllSecretsResponse>(&text);

                            match json_data {
                                Ok(d) => match d.deleted_count {
                                    0 => {
                                        spinner.stop_with_message("No secrets to delete.");
                                    }
                                    _ => {
                                        let msg = format!(
                                            "All secrets ({}) have been deleted!",
                                            d.deleted_count
                                        );

                                        spinner.stop_with_message(&format!(
                                            "{} {}",
                                            "✓".green(),
                                            msg
                                        ));
                                    }
                                },
                                Err(e) => {
                                    error!("{}", e);
                                    bail!("Something went wrong.");
                                }
                            }
                        }
                        None => {
                            bail!("Something went wrong.");
                        }
                    }
                }
                RequestApiOptionResponse::Err(e) => {
                    spinner.stop_with_message(&format!("\n{}", e));
                }
            }
        }
        false => {
            let res = secrets::delete(api_key, project, environment, &names).await;

            if let Err(err) = res {
                spinner.stop_and_persist("", "");
                error!("{:#?}", &err);
                bail!(err);
            }

            let res = res.unwrap();

            match res {
                RequestApiOptionResponse::Ok(res) => {
                    // all deleted
                    match res.text {
                        Some(text) => {
                            let json_data = serde_json::from_str::<DeleteSecretsResponse>(&text);
                            debug!("{:#?}", json_data);

                            match json_data {
                                Ok(data) => {
                                    let not_found_secrets = data.not_found_secrets;
                                    let not_found_len = not_found_secrets.len();

                                    debug!("{:#?}", not_found_secrets);

                                    if not_found_len > 0 {
                                        spinner.stop_and_persist("", "");

                                        let info_msg = format!(
                                            "{} {}",
                                            format!(
                                                "{} {} {}",
                                                "Secrets".red(),
                                                format!("({})", not_found_len).red(),
                                                "not found:".red()
                                            ),
                                            not_found_secrets.join(", ")
                                        );

                                        //
                                        eprintln!("{}", info_msg);

                                        let deleted_count = data.deleted_count;

                                        if deleted_count > 0 {
                                            let secrets_deleted: Vec<_> = names
                                                .into_iter()
                                                .filter(|k| {
                                                    not_found_secrets
                                                        .iter()
                                                        .find(|s| *s == k)
                                                        .is_none()
                                                })
                                                .collect();

                                            let msg = format!(
                                                "{} {}",
                                                format!(
                                                    "{} {} {}",
                                                    "Secrets".green(),
                                                    format!("({})", deleted_count).green(),
                                                    "deleted:".green()
                                                ),
                                                secrets_deleted.join(", ")
                                            );

                                            println!("{}", msg);
                                        }
                                    } else {
                                        // spinner.stop_with_message("🗑️ Selected secrets have been deleted!");
                                        spinner.stop_with_message(&format!(
                                            "{} {}",
                                            "✓".green(),
                                            "Selected secrets have been deleted."
                                        ));
                                    }
                                }
                                Err(e) => {
                                    error!("{}", e);
                                }
                            }
                        }
                        None => {
                            bail!("Something went wrong");
                        }
                    }
                }
                RequestApiOptionResponse::Err(e) => {
                    spinner.stop_with_message(&format!("\n{}", e));
                }
            }
        }
    }

    Ok(())
}

fn validate_input(project: &str, environment: &str, names: &Vec<String>) -> Result<()> {
    let name_is_valid = validate_project_name(project, false, false);

    if let Err(err) = name_is_valid {
        bail!(err);
    }

    let env_name_validation = validate_environment_name(environment, false, false);

    if let Err(err) = env_name_validation {
        bail!(err);
    }

    let names_valid = validate_secret_names(names);

    if let Err(err) = names_valid {
        debug!("Error: {:#?}", &err);
        bail!(err);
    }

    Ok(())
}
