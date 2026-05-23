use anyhow::{bail, Result};
use log::debug;

use crate::{
    api::secrets,
    cmd::config::SecretsOutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        secrets::{
            SecretMetadata, SecretMetadataListResponse, SecretMetadataTable,
            SecretMetadataTableWithoutComment,
        },
        validation::InputValidationError,
    },
    utils::{
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
        spinner::request_spinner,
        tables::build::build_table,
        validation::{validate_environment_name, validate_project_name, validate_secret_name},
    },
};

pub struct HandleListSecretMetadataArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub secret: Option<String>,
    pub format: SecretsOutputFormat,
    pub silent: bool,
}

pub struct HandleGetSecretMetadataArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub name: String,
    pub format: SecretsOutputFormat,
    pub silent: bool,
}

pub async fn handle_list_secret_metadata(args: HandleListSecretMetadataArgs) -> Result<()> {
    let HandleListSecretMetadataArgs {
        api_key,
        project,
        environment,
        secret,
        format,
        silent,
    } = args;

    validate_project_environment(&project, &environment, &format, silent)?;

    debug!("listing secret metadata...");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    if let Some(secret_name) = &secret {
        if let Err(err) = validate_secret_name(secret_name) {
            let error_output = err.format_error_output(format == SecretsOutputFormat::Json)?;

            if !silent {
                eprintln!();
            }

            bail!(error_output);
        }
    }

    let res = match secret {
        Some(secret_name) => {
            secrets::get_secret_metadata(api_key, project, environment, secret_name).await
        }
        None => secrets::list_secret_metadata(api_key, project, environment).await,
    };

    if let Some(mut spinner) = spinner {
        spinner.stop_and_persist("", "");
    }

    if let Err(err) = res {
        let error_output = err.format_error_output(format == SecretsOutputFormat::Json)?;
        bail!(error_output);
    }

    match res.unwrap() {
        GetRequestApiResponse::Ok(data) => {
            let parsed =
                serde_json::from_str::<SecretMetadataListResponse>(&data.text).or_else(|_| {
                    serde_json::from_str::<Vec<SecretMetadata>>(&data.text)
                        .map(|v| SecretMetadataListResponse { secrets: v })
                }).or_else(|_| {
                    serde_json::from_str::<SecretMetadata>(&data.text)
                        .map(|v| SecretMetadataListResponse { secrets: vec![v] })
                });

            let payload = match parsed {
                Ok(v) => v,
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    bail!(error.format_error_output(format == SecretsOutputFormat::Json)?);
                }
            };

            print_secret_metadata_list(payload.secrets, &format);
        }
        GetRequestApiResponse::Err(e) => {
            let error_output = e.format_error_output(format == SecretsOutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}

pub async fn handle_get_secret_metadata(args: HandleGetSecretMetadataArgs) -> Result<()> {
    let HandleGetSecretMetadataArgs {
        api_key,
        project,
        environment,
        name,
        format,
        silent,
    } = args;

    validate_project_environment(&project, &environment, &format, silent)?;

    if let Err(err) = validate_secret_name(&name) {
        let error_output = err.format_error_output(format == SecretsOutputFormat::Json)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    debug!("getting secret metadata...");

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let res = secrets::get_secret_metadata(api_key, project, environment, name).await;

    if let Some(mut spinner) = spinner {
        spinner.stop_and_persist("", "");
    }

    if let Err(err) = res {
        let error_output = err.format_error_output(format == SecretsOutputFormat::Json)?;
        bail!(error_output);
    }

    match res.unwrap() {
        GetRequestApiResponse::Ok(data) => {
            let payload = serde_json::from_str::<SecretMetadata>(&data.text);

            match payload {
                Ok(v) => print_secret_metadata(v, &format),
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    bail!(error.format_error_output(format == SecretsOutputFormat::Json)?);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            let error_output = e.format_error_output(format == SecretsOutputFormat::Json)?;
            bail!(error_output);
        }
    }

    Ok(())
}

fn validate_project_environment(
    project: &Option<String>,
    environment: &Option<String>,
    format: &SecretsOutputFormat,
    silent: bool,
) -> Result<()> {
    if project.is_some() && environment.is_some() {
        if let Err(err) = validate_project_name(project.as_ref().unwrap(), false, false) {
            return format_validation_err(err, format, silent);
        }

        if let Err(err) = validate_environment_name(environment.as_ref().unwrap(), false, false) {
            return format_validation_err(err, format, silent);
        }
    }

    Ok(())
}

fn format_validation_err(
    err: InputValidationError,
    format: &SecretsOutputFormat,
    silent: bool,
) -> Result<()> {
    let error_output = err.format_error_output(*format == SecretsOutputFormat::Json)?;

    if !silent {
        eprintln!();
    }

    bail!(error_output)
}

fn print_secret_metadata_list(secret_metadata: Vec<SecretMetadata>, format: &SecretsOutputFormat) {
    match format {
        SecretsOutputFormat::Json => {
            let pretty = get_formatted_json_string(&secret_metadata, true).unwrap();
            println!("{}", pretty);
        }
        SecretsOutputFormat::Yaml => {
            let value = serde_yaml::to_string(&secret_metadata).unwrap();
            println!("{}", value);
        }
        SecretsOutputFormat::Table => {
            if secret_metadata.is_empty() {
                return;
            }

            let has_any_comment = secret_metadata
                .iter()
                .any(|s| s.comment.as_ref().map(|c| !c.trim().is_empty()).unwrap_or(false));

            if has_any_comment {
                let rows = secret_metadata
                    .into_iter()
                    .map(SecretMetadataTable::from)
                    .collect::<Vec<_>>();
                println!("{}", build_table(&rows));
            } else {
                let rows = secret_metadata
                    .into_iter()
                    .map(SecretMetadataTableWithoutComment::from)
                    .collect::<Vec<_>>();
                println!("{}", build_table(&rows));
            }
        }
        SecretsOutputFormat::List | SecretsOutputFormat::Dotenv => {
            if secret_metadata.is_empty() {
                return;
            }

            let body = secret_metadata
                .iter()
                .map(|s| {
                    let comment = s.comment.clone().unwrap_or_default();
                    let last_accessed_at = s.last_accessed_at.clone().unwrap_or_default();

                    format!(
                        "{}\n{} {}\n{} {}\n{} {}\n{} {}\n{} {}\n{} {}",
                        s.name.clone().green_if_tty(),
                        "comment:".blue_if_tty(),
                        comment,
                        "version:".blue_if_tty(),
                        s.version,
                        "has_value:".blue_if_tty(),
                        s.has_value,
                        "created_at:".blue_if_tty(),
                        s.created_at,
                        "updated_at:".blue_if_tty(),
                        s.updated_at,
                        "last_accessed_at:".blue_if_tty(),
                        last_accessed_at,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            println!("{}", body);
        }
    }
}

fn print_secret_metadata(secret_metadata: SecretMetadata, format: &SecretsOutputFormat) {
    match format {
        SecretsOutputFormat::Json => {
            let pretty = get_formatted_json_string(&secret_metadata, true).unwrap();
            println!("{}", pretty);
        }
        SecretsOutputFormat::Yaml => {
            let value = serde_yaml::to_string(&secret_metadata).unwrap();
            println!("{}", value);
        }
        SecretsOutputFormat::Table => {
            let has_comment = secret_metadata
                .comment
                .as_ref()
                .map(|c| !c.trim().is_empty())
                .unwrap_or(false);

            if has_comment {
                println!(
                    "{}",
                    build_table(&vec![SecretMetadataTable::from(secret_metadata)])
                );
            } else {
                println!(
                    "{}",
                    build_table(&vec![SecretMetadataTableWithoutComment::from(
                        secret_metadata
                    )])
                );
            }
        }
        SecretsOutputFormat::List | SecretsOutputFormat::Dotenv => {
            let comment = secret_metadata.comment.unwrap_or_default();
            let last_accessed_at = secret_metadata.last_accessed_at.unwrap_or_default();

            println!(
                "{}\n{} {}\n{} {}\n{} {}\n{} {}\n{} {}\n{} {}",
                secret_metadata.name.green_if_tty(),
                "comment:".blue_if_tty(),
                comment,
                "version:".blue_if_tty(),
                secret_metadata.version,
                "has_value:".blue_if_tty(),
                secret_metadata.has_value,
                "created_at:".blue_if_tty(),
                secret_metadata.created_at,
                "updated_at:".blue_if_tty(),
                secret_metadata.updated_at,
                "last_accessed_at:".blue_if_tty(),
                last_accessed_at,
            );
        }
    }
}
