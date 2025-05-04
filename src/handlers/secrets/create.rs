use anyhow::{bail, Result};
use log::debug;
use owo_colors::OwoColorize;

use crate::{
    api::secrets,
    models::{
        api_client::RequestApiOptionResponse,
        secrets::{CreateSecretsResponse, Secret, ValidateSecrets},
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        interaction, output::get_colored_json, secrets::format_secret_comment, separator,
        spinner::request_spinner,
    },
};

pub struct HandleCreateSecretsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub values: Vec<String>,
    pub comments: Vec<String>,
    pub json_format: bool,
}

pub async fn handle_create_secrets(args: HandleCreateSecretsArgs) -> Result<()> {
    let HandleCreateSecretsArgs {
        api_key,
        project,
        environment,
        values,
        comments,
        json_format,
    } = args;

    if values.is_empty() {
        bail!("{} No secrets to create provided", "Input error:".red());
    }

    let name_value_pairs = separator::key_value(values);

    debug!("{:#?}", name_value_pairs);

    if let Err(_) = name_value_pairs {
        eprintln!();

        let error = InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);
        let error_output = error.format_error_output(json_format)?;

        bail!(error_output);
    }

    let name_value_pairs = name_value_pairs.unwrap();

    let comment_pairs = separator::key_value(comments);
    debug!("{:#?}", comment_pairs);

    if let Err(_) = comment_pairs {
        let error = InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);
        let error_output = error.format_error_output(json_format)?;

        eprintln!();
        bail!(error_output);
    }

    // OK
    let comment_pairs = comment_pairs.unwrap();
    let mut payload = Vec::new();

    for x in name_value_pairs {
        let comment = comment_pairs.iter().find(|d| d.0 == x.0);

        let secret = match comment {
            Some((_, c_value)) => {
                let formatted_comment = match c_value.is_empty() {
                    true => "".to_string(),
                    false => format_secret_comment(&c_value.to_string(), true),
                };

                Secret {
                    name: x.0,
                    value: x.1,
                    comment: Some(formatted_comment),
                }
            }
            None => Secret {
                name: x.0,
                value: x.1,
                comment: None,
            },
        };

        payload.push(secret);
    }

    if let Err(err) = payload.validate() {
        let error = InputValidationError::Secrets(SecretsInputValidationError::NameValueSeparator);
        let error_output = error.format_error_output(json_format)?;

        eprintln!();
        bail!(error_output);
    }

    let reference_warnings = payload.get_reference_warnings();

    if !reference_warnings.is_empty() {
        eprint!("{}", reference_warnings);

        let confirm = interaction::confirm_opt("Are you sure you want to continue?");

        if confirm.is_none() || (confirm.unwrap() == false) {
            return Ok(());
        }
    }

    let mut spinner = request_spinner();
    let res = secrets::create_secrets(api_key, project, environment, &payload).await;

    if let Err(err) = res {
        spinner.stop_and_persist("", "");
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let res = res.unwrap();

    match res {
        RequestApiOptionResponse::Ok(res) => match res.text {
            Some(text) => {
                let json_data = serde_json::from_str::<CreateSecretsResponse>(&text);

                match json_data {
                    Ok(data) => {
                        if json_format {
                            let json_str = get_colored_json(&data).unwrap();

                            spinner.stop_and_persist("", "");
                            println!("{}", json_str);

                            return Ok(());
                        }

                        let created_count = data.created_count;
                        let duplicate_secrets = data.duplicate_secrets;

                        if duplicate_secrets.len() > 0 {
                            if created_count > 0 {
                                spinner.stop_and_persist("", "");

                                let secrets_created: Vec<_> = payload
                                    .into_iter()
                                    .filter(|k| {
                                        duplicate_secrets.iter().find(|s| *s == &k.name).is_none()
                                    })
                                    .collect();

                                let msg = format!(
                                    "{} {}",
                                    format!(
                                        "{} {} {}",
                                        "Secrets".green(),
                                        "created".green(),
                                        format!("({}):", created_count).green(),
                                    ),
                                    secrets_created
                                        .iter()
                                        .map(|s| s.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );

                                println!("{}", msg);
                            } else {
                                if created_count == 0 && duplicate_secrets.len() == 0 {
                                    spinner.stop_and_persist("No secrets created.", "");
                                } else {
                                    spinner.stop_and_persist("", "");
                                    let msg = format!("No secrets created.");
                                    println!("{}", msg);
                                }
                            }

                            let info_msg = format!(
                                "{} {}",
                                format!(
                                    "{} {} {}",
                                    "Secrets".red(),
                                    "already exist".red(),
                                    format!("({}):", duplicate_secrets.len()).red(),
                                ),
                                duplicate_secrets.join(", ")
                            );

                            //
                            eprintln!("{}", info_msg);
                        } else {
                            // spinner.stop_with_message("🗑️ Selected secrets have been deleted!");
                            spinner.stop_with_message("Secrets created.");
                        }
                    }
                    Err(e) => {
                        spinner.stop_and_persist("", "");
                        bail!(e);
                    }
                }
            }
            None => {
                spinner.stop_and_persist("", "");
                bail!("Something went wrong");
            }
        },
        RequestApiOptionResponse::Err(e) => {
            debug!("Error: {}", e);
            spinner.stop_and_persist("", "");

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}
