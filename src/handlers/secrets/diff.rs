use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Result};
use log::{debug, error};

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        secrets::{
            Secret, SecretDiffModified, SecretDiffModifiedChange, SecretDiffModifiedChangeItem,
            SecretOptional, SecretsDiff,
        },
        validation::{InputValidationError, SecretsInputValidationError},
    },
    utils::{
        self, output::get_formatted_json_string, secrets::read_secrets_from_file,
        spinner::request_spinner,
    },
};

pub struct HandleSecretsDiffArgs {
    pub silent: bool,
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub file_path: String,
    pub format: Option<SecretsFileFormat>,
    pub json_format: bool,
    pub expand_refs: bool,
    pub with_comments: bool,
}

pub async fn handle_secrets_diff(args: HandleSecretsDiffArgs) -> Result<()> {
    let HandleSecretsDiffArgs {
        silent,
        api_key,
        project,
        environment,
        file_path,
        format,
        json_format,
        expand_refs,
        with_comments,
    } = args;

    let path = Path::new(&file_path);

    let file_exists = path.exists();

    if !file_exists {
        // TODO: validation error
        bail!("File does not exist: {}", file_path);
    }

    let file_format = match format {
        Some(f) => f,
        None => {
            if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
                SecretsFileFormat::Yaml
            } else if file_path.ends_with(".json") {
                SecretsFileFormat::Json
            } else {
                SecretsFileFormat::Dotenv
            }
        }
    };

    let secrets_res = read_secrets_from_file(path, &file_format);

    if let Err(err) = secrets_res {
        let err =
            InputValidationError::Secrets(SecretsInputValidationError::ReadFile(err.to_string()));

        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(error_output);
    }

    let mut secrets = secrets_res.unwrap();

    if expand_refs == true {
        utils::secrets::expand_secret_references(&mut secrets);
    }

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let remote_secrets_res =
        secrets::list(api_key, project, environment, false, None, expand_refs).await;

    if let Err(err) = remote_secrets_res {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }
        debug!("Error: {:#?}", &err);

        let error_output = err.format_error_output(json_format)?;
        bail!(error_output);
    }

    let remote_secrets = remote_secrets_res.unwrap();

    match remote_secrets {
        GetRequestApiResponse::Ok(data) => {
            let names = serde_json::from_str::<Vec<SecretOptional>>(&data.text);

            match names {
                Ok(remote_secrets) => {
                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }

                    let diff = create_secrets_diff(secrets, remote_secrets, with_comments);

                    if json_format {
                        let json_str = get_formatted_json_string(&diff, true)?;
                        println!("{}", json_str);
                    } else {
                        print!("{}", diff);
                    }
                }
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    if let Some(mut spinner) = spinner {
                        spinner.stop_and_persist("", "");
                    }
                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(e) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let error_output = e.format_error_output(json_format)?;
            bail!(error_output);
        }
    }

    Ok(())
}

/// Convert two arrays of secrets into a diff
/// - `local_secrets`: secrets from the local file
/// - `remote_secrets`: secrets from the remote environment
pub fn create_secrets_diff(
    local_secrets: Vec<Secret>,
    remote_secrets: Vec<SecretOptional>,
    with_comments: bool,
) -> SecretsDiff {
    // Create HashMaps for efficient lookup by name
    let local_map: HashMap<String, &Secret> = local_secrets
        .iter()
        .map(|secret| (secret.name.clone(), secret))
        .collect();

    let remote_map: HashMap<String, &SecretOptional> = remote_secrets
        .iter()
        .map(|secret| (secret.name.clone(), secret))
        .collect();

    let mut added = Vec::new();
    let mut missing = Vec::new();
    let mut modified = Vec::new();

    // Find added secrets (exist in local but not in remote)
    for local_secret in &local_secrets {
        if !remote_map.contains_key(&local_secret.name) {
            added.push(SecretOptional {
                name: local_secret.name.clone(),
                value: Some(local_secret.value.clone()),
                comment: if with_comments {
                    local_secret.comment.clone()
                } else {
                    None
                },
            });
        }
    }

    // Find missing secrets (exist in remote but not in local)
    for remote_secret in &remote_secrets {
        if !local_map.contains_key(&remote_secret.name) {
            missing.push((*remote_secret).clone());
        }
    }

    // Find modified secrets (exist in both but have different values or comments)
    for local_secret in &local_secrets {
        if let Some(remote_secret) = remote_map.get(&local_secret.name) {
            // Check if values or comments are different
            let values_differ = local_secret.value != remote_secret.value.clone().unwrap();
            let comments_differ = with_comments && local_secret.comment != remote_secret.comment;

            if values_differ || comments_differ {
                let changes = Some(SecretDiffModifiedChange {
                    local: SecretDiffModifiedChangeItem {
                        value: Some(local_secret.value.clone()),
                        comment: local_secret.comment.clone(),
                    },
                    remote: SecretDiffModifiedChangeItem {
                        value: remote_secret.value.clone(),
                        comment: remote_secret.comment.clone(),
                    },
                });

                modified.push(SecretDiffModified {
                    name: local_secret.name.clone(),
                    changes,
                });
            }
        }
    }

    // Sort all vectors by name
    added.sort_by(|a, b| a.name.cmp(&b.name));
    missing.sort_by(|a, b| a.name.cmp(&b.name));
    modified.sort_by(|a, b| a.name.cmp(&b.name));

    SecretsDiff {
        added,
        missing,
        modified,
    }
}
