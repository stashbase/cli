use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use linked_hash_map::LinkedHashMap;
use linked_hash_set::LinkedHashSet;
use regex::Regex;
use short_uuid::ShortUuid;

use crate::models::{
    secrets::{
        ReferencesValidation, ReferencesValidationWithExistence, Secret, SecretReferenceWarnings,
    },
    validation::{
        EnvChangelogInputValidationError, EnvironmentsInputValidationError, InputValidationError,
        LoadEnvironmentInputValidationError, ProjectInputValidationError,
        SecretsInputValidationError, WebhookInputValidationError,
    },
};

use super::secrets::{self, format_secret_description};

// 512 is max length for description after formatting
pub const SECRET_DESCRIPTION_MAX_LENGTH: usize = 512;
// 4096 is max length for value after formatting
pub const SECRET_VALUE_MAX_LENGTH: usize = 4096;
// 2 is min length for secret name
pub const SECRET_NAME_MIN_LENGTH: usize = 2;
// 255 is max length for secret name
pub const SECRET_NAME_MAX_LENGTH: usize = 255;

pub fn count_dashes(s: &str) -> usize {
    s.chars().filter(|&c| c == '-').count()
}

pub fn validate_project_name(value: &str, is_new_name: bool, is_root: bool) -> Result<()> {
    if value.len() < 2 {
        let err = if is_new_name {
            InputValidationError::Projects(ProjectInputValidationError::NewNameTooShort)
        } else {
            InputValidationError::Projects(ProjectInputValidationError::NameTooShort { is_root })
        };

        bail!(err)
    }

    if value.len() > 40 {
        let err = if is_new_name {
            InputValidationError::Projects(ProjectInputValidationError::NewNameTooLong)
        } else {
            InputValidationError::Projects(ProjectInputValidationError::NameTooLong { is_root })
        };

        bail!(err)
    }

    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if !regex.is_match(value) {
        let err = if is_new_name {
            InputValidationError::Projects(ProjectInputValidationError::NewNameFormat)
        } else {
            InputValidationError::Projects(ProjectInputValidationError::NameFormat { is_root })
        };

        bail!(err)
    }

    Ok(())
}

pub fn validate_project_identifier(value: &str, is_root: bool) -> Result<()> {
    if value.len() < 2 || value.len() > 40 {
        let err =
            InputValidationError::Projects(ProjectInputValidationError::InvalidIdentifierFormat {
                is_root,
            });

        bail!(err)
    }

    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if !regex.is_match(value) {
        let err =
            InputValidationError::Projects(ProjectInputValidationError::InvalidIdentifierFormat {
                is_root,
            });

        bail!(err)
    }

    Ok(())
}

pub enum IdentifierResource {
    Project,
    Environment,
}

pub fn resource_name_has_id_format(resource: IdentifierResource, input: &str) -> bool {
    let prefix = match resource {
        IdentifierResource::Project => "proj_",
        IdentifierResource::Environment => "env_",
    };

    if !input.starts_with(prefix) {
        return false;
    }

    let id_without_prefix = &input[prefix.len()..];

    if id_without_prefix.len() != 22 {
        return false;
    }

    let alphanumeric_regex = regex::Regex::new(r"^[a-zA-Z0-9]+$").unwrap();
    alphanumeric_regex.is_match(id_without_prefix)
}

// name of secret
pub fn validate_secret_name(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();
    let starts_with_digit = value.chars().nth(0).unwrap_or(' ').is_ascii_digit();

    if !regex.is_match(value) || starts_with_digit {
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameFormat(vec![
            value.to_string(),
        ]));

        bail!(err)
    }

    if value.len() < SECRET_NAME_MIN_LENGTH {
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameTooShort(vec![
            value.to_string(),
        ]));

        bail!(err)
    }

    if value.len() > SECRET_NAME_MAX_LENGTH {
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameTooLong(vec![
            value.to_string(),
        ]));

        bail!(err)
    }

    Ok(())
}

pub fn validate_secret_names(values: &Vec<String>) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    let (invalid_format_names, too_short_names, too_long_names): (
        HashSet<String>,
        HashSet<String>,
        HashSet<String>,
    ) = values.into_iter().fold(
        (HashSet::new(), HashSet::new(), HashSet::new()),
        |(mut invalid_format_names, mut too_short_names, mut too_long_names), x| {
            if !regex.is_match(&x) || x.chars().nth(0).unwrap_or(' ').is_ascii_digit() {
                invalid_format_names.insert(x.clone());
            } else if x.len() < SECRET_NAME_MIN_LENGTH {
                too_short_names.insert(x.clone());
            } else if x.len() > SECRET_NAME_MAX_LENGTH {
                too_long_names.insert(x.clone());
            }
            (invalid_format_names, too_short_names, too_long_names)
        },
    );

    if invalid_format_names.len() > 0 {
        let invalid_format_names_vec = invalid_format_names.into_iter().collect();
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameFormat(
            invalid_format_names_vec,
        ));

        bail!(err)
    }

    if too_short_names.len() > 0 {
        let too_short_names_vec = too_short_names.into_iter().collect();
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameTooShort(
            too_short_names_vec,
        ));

        bail!(err)
    }

    if too_long_names.len() > 0 {
        let too_long_names_vec = too_long_names.into_iter().collect();
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameTooLong(
            too_long_names_vec,
        ));

        bail!(err)
    }

    // let invalid = values.into_iter().find(|v| !regex.is_match(*v));
    //
    // if invalid.is_some() {
    //     let multiple = values.len() > 1;
    //     let err =
    //         InputValidationError::Secrets(SecretsInputValidationError::KeyFormat { multiple });
    //
    //     bail!(err)
    // }

    Ok(())
}

pub fn validate_secret_values(values: &Vec<String>) -> Result<()> {
    let too_long_value_secret_names: Vec<_> = values
        .iter()
        .filter(|value| value.len() > SECRET_VALUE_MAX_LENGTH)
        .map(|v| v.to_string())
        .collect();

    if too_long_value_secret_names.len() > 0 {
        let error_message = SecretsInputValidationError::ValuesTooLong(too_long_value_secret_names);
        let err = InputValidationError::Secrets(error_message);
        bail!(err);
    }

    Ok(())
}

// self reference = fatal error, invalid format = warning
pub fn validate_secrets_references(
    // secrets: &Vec<(String, String)>,
    secrets: &Vec<Secret>,
) -> ReferencesValidation {
    let mut self_referenced_secrets: HashSet<_> = HashSet::new();
    let mut invalid_format_secrets: LinkedHashMap<String, Vec<String>> = LinkedHashMap::new();

    for Secret {
        name,
        value,
        description: _,
    } in secrets
    {
        let all_unique_refs = secrets::extract_unique_references_from_secret(&value);
        let has_self_reference = all_unique_refs.contains(name);

        if has_self_reference {
            self_referenced_secrets.insert(name.clone());
        }

        for ref_ in all_unique_refs {
            let is_valid_secret_name = validate_secret_name(&ref_).is_ok();

            if !is_valid_secret_name {
                if !self_referenced_secrets.contains(&ref_) {
                    invalid_format_secrets
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(ref_);
                }
            }
        }
    }

    let validation_obj =
        ReferencesValidation::new(Some(self_referenced_secrets), Some(invalid_format_secrets));
    validation_obj
}

// self reference = fatal error, invalid format = warning
pub fn validate_secrets_references_with_existence(
    secrets: &Vec<Secret>,
) -> ReferencesValidationWithExistence {
    let mut validation_obj = ReferencesValidationWithExistence::new();

    let mut secret_names = HashSet::new();

    for secret in secrets {
        secret_names.insert(secret.name.to_owned());
    }

    for Secret {
        name,
        value,
        description: _,
    } in secrets
    {
        let all_unique_refs = secrets::extract_unique_references_from_secret(&value);
        let has_self_reference = all_unique_refs.contains(name);

        if has_self_reference {
            validation_obj.self_referenced_secrets.push(name.clone());
        }

        for ref_ in all_unique_refs {
            let is_valid_secret_name = validate_secret_name(&ref_).is_ok();

            if !is_valid_secret_name {
                if !validation_obj.self_referenced_secrets.contains(&ref_) {
                    validation_obj
                        .invalid_format
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(ref_);
                }
            } else {
                if !secret_names.contains(&ref_) {
                    validation_obj
                        .not_found
                        .entry(name.clone())
                        .or_insert_with(Vec::new)
                        .push(ref_);
                }
            }
        }
    }

    validation_obj
}

pub fn get_secrets_reference_warnings(secrets: &Vec<Secret>) -> SecretReferenceWarnings {
    let mut validation_obj = SecretReferenceWarnings::new();
    let secret_names: HashSet<_> = secrets.iter().map(|s| &s.name).collect();

    for secret in secrets {
        let all_unique_refs = secrets::extract_unique_references_from_secret(&secret.value);

        for ref_ in all_unique_refs {
            if ref_.trim().is_empty() {
                validation_obj
                    .empty_value
                    .insert_if_absent(secret.name.clone());
                continue;
            }

            if !validate_secret_name(&ref_).is_ok() {
                validation_obj
                    .invalid_format
                    .entry(secret.name.clone())
                    .or_insert_with(Vec::new)
                    .push(ref_);
            } else if !secret_names.contains(&ref_) {
                validation_obj
                    .not_found
                    .entry(secret.name.clone())
                    .or_insert_with(Vec::new)
                    .push(ref_);
            }
        }
    }

    validation_obj
}

pub fn validate_secret_name_new_name(values: &Vec<(String, String)>) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    let invalid = values
        .into_iter()
        .find(|k| !regex.is_match(&k.0) || !regex.is_match(&k.1));

    if let Some((name, new_name)) = invalid {
        let vec = vec![name.clone(), new_name.clone()];
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameFormat(vec));

        bail!(err)
    }

    Ok(())
}

pub fn is_valid_secret_description(formatted_description: &str) -> bool {
    formatted_description.len() <= SECRET_DESCRIPTION_MAX_LENGTH
}

// takes mutable reference of secrets
pub fn format_secrets_input(secrets: &mut Vec<Secret>) {
    for secret in secrets.iter_mut() {
        if let Some(ref d) = secret.description {
            secret.description = Some(if d.trim().is_empty() {
                String::new()
            } else {
                format_secret_description(&d, true)
            });
        }
    }
}

pub fn validate_secrets(secrets: &Vec<Secret>) -> Result<()> {
    let mut invalid_names = LinkedHashSet::new();
    let mut self_references = LinkedHashSet::new();
    let mut description_too_long_secrets_names = LinkedHashSet::new();
    let mut value_too_long_secret_names = LinkedHashSet::new();
    let mut name_counts = HashMap::new();

    // First pass: collect references to invalid secrets
    for secret in secrets {
        let name = &secret.name;

        // Validate name format
        if validate_secret_name(name).is_err() {
            invalid_names.insert_if_absent(name);
        }

        // Check for self references
        if secret.value.contains(&format!("${{{}}}", name)) {
            self_references.insert_if_absent(name);
        }

        // Track name occurrences for duplicates
        *name_counts.entry(name).or_insert(0) += 1;

        // Check value length
        if secret.value.len() > SECRET_VALUE_MAX_LENGTH {
            value_too_long_secret_names.insert_if_absent(name);
        }

        // Check description length if present
        if let Some(desc) = &secret.description {
            if desc.len() > SECRET_DESCRIPTION_MAX_LENGTH {
                description_too_long_secrets_names.insert_if_absent(name);
            }
        }
    }

    // Only clone strings when constructing the final error
    if !invalid_names.is_empty() {
        let invalid_names_vec = invalid_names.into_iter().map(|s| s.to_string()).collect();
        let err = InputValidationError::Secrets(SecretsInputValidationError::NameFormat(
            invalid_names_vec,
        ));

        bail!(err);
    }

    // Find duplicates
    let duplicate_names: Vec<_> = name_counts
        .iter()
        .filter(|(_, &count)| count > 1)
        .map(|(name, _)| (*name).to_string())
        .collect();

    if !duplicate_names.is_empty() {
        let duplicate_names_vec = duplicate_names.into_iter().map(|s| s.to_string()).collect();
        let err = InputValidationError::Secrets(SecretsInputValidationError::DuplicateNames(
            duplicate_names_vec,
        ));
        bail!(err);
    }

    if !value_too_long_secret_names.is_empty() {
        let value_too_long_secret_names_vec = value_too_long_secret_names
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let err = InputValidationError::Secrets(SecretsInputValidationError::ValuesTooLong(
            value_too_long_secret_names_vec,
        ));
        bail!(err);
    }

    if !description_too_long_secrets_names.is_empty() {
        let description_too_long_secrets_names_vec = description_too_long_secrets_names
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let err = InputValidationError::Secrets(SecretsInputValidationError::DescriptionTooLong(
            description_too_long_secrets_names_vec,
        ));
        bail!(err);
    }

    if !self_references.is_empty() {
        let self_references_vec = self_references.into_iter().map(|s| s.to_string()).collect();

        let err = InputValidationError::Secrets(SecretsInputValidationError::SelfReferences(
            self_references_vec,
        ));
        bail!(err);
    }

    Ok(())
}

pub fn validate_environment_name(value: &str, is_new_name: bool, is_root: bool) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+(?:/[a-zA-Z0-9-_]+)?$").unwrap();

    if value.len() < 2 {
        let err = if is_new_name == false {
            InputValidationError::Environments(EnvironmentsInputValidationError::NameTooShort {
                is_root,
            })
        } else {
            InputValidationError::Environments(EnvironmentsInputValidationError::NewNameTooShort)
        };

        bail!(err)
    }

    if value.len() > 40 {
        let err = if is_new_name == false {
            InputValidationError::Environments(EnvironmentsInputValidationError::NameTooLong {
                is_root,
            })
        } else {
            InputValidationError::Environments(EnvironmentsInputValidationError::NewNameTooLong)
        };

        bail!(err)
    }

    let dash_count = count_dashes(value);
    let is_firt_dash = value.chars().nth(0) == Some('-');
    let is_last_dash = value.chars().nth(value.len() - 1) == Some('-');

    if !regex.is_match(value) || dash_count > 1 || is_firt_dash || is_last_dash {
        let err = if is_new_name == false {
            InputValidationError::Environments(EnvironmentsInputValidationError::NameFormat {
                is_root,
            })
        } else {
            InputValidationError::Environments(EnvironmentsInputValidationError::NewNameFormat)
        };

        bail!(err)
    }

    Ok(())
}

pub fn validate_environment_identifier(value: &str, is_root: bool) -> Result<()> {
    if value.len() < 2 || value.len() > 40 {
        let err = InputValidationError::Environments(
            EnvironmentsInputValidationError::InvalidIdentifierFormat { is_root },
        );

        bail!(err)
    } else {
        let regex = Regex::new(r"^[a-zA-Z0-9-_]+(?:/[a-zA-Z0-9-_]+)?$").unwrap();

        let dash_count = count_dashes(value);
        let is_firt_dash = value.chars().nth(0) == Some('-');
        let is_last_dash = value.chars().nth(value.len() - 1) == Some('-');

        if !regex.is_match(value) || dash_count > 1 || is_firt_dash || is_last_dash {
            let err = InputValidationError::Environments(
                EnvironmentsInputValidationError::InvalidIdentifierFormat { is_root },
            );

            bail!(err)
        }
    }

    Ok(())
}

pub fn validate_project_environment(
    project: &str,
    environment: &str,
    env_is_root: bool,
) -> Result<()> {
    let project_name_is_valid = validate_project_name(project, false, false);

    if let Err(err) = project_name_is_valid {
        bail!(err);
    }

    // validate env
    let env_name_is_valid = validate_environment_name(environment, false, env_is_root);

    if let Err(err) = env_name_is_valid {
        bail!(err);
    }

    Ok(())
}

pub fn validate_project_environment_identifier(
    project: &str,
    environment: &str,
    env_is_root: bool,
) -> Result<()> {
    let project_name_is_valid = validate_project_identifier(project, false);

    if let Err(err) = project_name_is_valid {
        bail!(err);
    }

    // validate env
    let env_name_is_valid = validate_environment_identifier(environment, env_is_root);

    if let Err(err) = env_name_is_valid {
        bail!(err);
    }

    Ok(())
}

//
pub fn validate_env_search(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+(?:/[a-zA-Z0-9-_]+)?$").unwrap();

    if value.len() < 2 {
        let err =
            InputValidationError::Environments(EnvironmentsInputValidationError::SearchTooShort);

        bail!(err)
    } else {
        let dash_count = count_dashes(value);
        let is_firt_dash = value.chars().nth(0) == Some('-');

        if !regex.is_match(value) || dash_count > 1 || is_firt_dash {
            let err =
                InputValidationError::Environments(EnvironmentsInputValidationError::SearchFormat);

            bail!(err)
        }
    }

    Ok(())
}

pub fn validate_project_search(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if value.len() < 2 {
        let err = InputValidationError::Projects(ProjectInputValidationError::SearchTooShort);

        bail!(err)
    } else {
        if !regex.is_match(value) {
            let err = InputValidationError::Projects(ProjectInputValidationError::SearchFormat);

            bail!(err)
        }
    }

    Ok(())
}

pub fn validate_secret_search(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    if value.len() < 2 {
        let err = InputValidationError::Secrets(SecretsInputValidationError::SearchTooShort);
        bail!(err)
    }

    if !regex.is_match(value) {
        let err = InputValidationError::Secrets(SecretsInputValidationError::SearchFormat);
        bail!(err)
    }

    Ok(())
}

pub fn validate_env_changelog_id(value: &str) -> Result<()> {
    let prefix = "chng_";

    if (value.starts_with(prefix)) == false {
        let input_err = EnvChangelogInputValidationError::InvalidId;
        bail!(InputValidationError::EnvChangelog(input_err));
    }

    let id_without_prefix = &value[prefix.len()..];

    let parsed = ShortUuid::parse_str(id_without_prefix);

    if let Err(_) = parsed {
        let input_err = EnvChangelogInputValidationError::InvalidId;
        bail!(InputValidationError::EnvChangelog(input_err));
    }

    // let regex = Regex::new(r"^[a-zA-Z0-9]+$").unwrap();
    //
    // if value.len() != 22 {
    //     let err =
    //         InputValidationError::EnvChangelog(EnvChangelogInputValidationError::InvalidIdLength);
    //
    //     bail!(err)
    // } else {
    //     if !regex.is_match(value) {
    //         InputValidationError::EnvChangelog(EnvChangelogInputValidationError::InvalidIdFormat);
    //     }
    // }

    Ok(())
}

pub fn validate_webhook_id(value: &str) -> Result<()> {
    let prefix = "whk_";

    if (value.starts_with(prefix)) == false {
        let input_err = WebhookInputValidationError::InvalidId;
        bail!(InputValidationError::Webhook(input_err));
    }

    let parsed = ShortUuid::parse_str(&value.strip_prefix(prefix).unwrap());

    if let Err(_) = parsed {
        let input_err = WebhookInputValidationError::InvalidId;

        bail!(InputValidationError::Webhook(input_err));
    }

    Ok(())
}

pub fn validate_webhook_url(url: &str) -> Result<()> {
    let https_url_regex = Regex::new(r"^(https://[-\w]+(\.\w[-\w]*)+)([/?].*)?$").unwrap();

    if !https_url_regex.is_match(url) {
        let input_err = WebhookInputValidationError::InvalidUrl;
        bail!(InputValidationError::Webhook(input_err));
    }

    Ok(())
}

pub fn validate_webhook_description(description: &str) -> Result<()> {
    if description.len() > 200 {
        let input_err = WebhookInputValidationError::DescriptionTooLong;
        bail!(InputValidationError::Webhook(input_err));
    }

    Ok(())
}

// Maps secret validation errors to load environment exclude secrets errors.
///
/// # Arguments
/// * `err` - The input validation error to map
///
/// # Panics
/// Panics if the error is not a secret validation error or is an unexpected secret error type.
pub fn map_secret_to_load_exclude_secrets_error(
    err: &InputValidationError,
) -> LoadEnvironmentInputValidationError {
    match err {
        InputValidationError::Secrets(secret_err) => match secret_err {
            SecretsInputValidationError::NameFormat(secret_names) => {
                LoadEnvironmentInputValidationError::ExcludeSecretNameFormat(secret_names.clone())
            }
            SecretsInputValidationError::NameTooShort(secret_names) => {
                LoadEnvironmentInputValidationError::ExcludeSecretNameTooShort(secret_names.clone())
            }
            SecretsInputValidationError::NameTooLong(secret_names) => {
                LoadEnvironmentInputValidationError::ExcludeSecretNameTooLong(secret_names.clone())
            }
            other => unreachable!("Unexpected secret validation error: {:?}", other),
        },
        other => unreachable!("Expected Secrets validation error, got: {:?}", other),
    }
}

// Maps secret validation errors to load environment only secrets errors.
///
/// # Arguments
/// * `err` - The input validation error to map
///
/// # Panics
/// Panics if the error is not a secret validation error or is an unexpected secret error type.
pub fn map_secret_to_load_only_secrets_error(
    err: &InputValidationError,
) -> LoadEnvironmentInputValidationError {
    match err {
        InputValidationError::Secrets(secret_err) => match secret_err {
            SecretsInputValidationError::NameFormat(secret_names) => {
                LoadEnvironmentInputValidationError::OnlySecretNameFormat(secret_names.clone())
            }
            SecretsInputValidationError::NameTooShort(secret_names) => {
                LoadEnvironmentInputValidationError::OnlySecretNameTooShort(secret_names.clone())
            }
            SecretsInputValidationError::NameTooLong(secret_names) => {
                LoadEnvironmentInputValidationError::OnlySecretNameTooLong(secret_names.clone())
            }
            other => unreachable!("Unexpected secret validation error: {:?}", other),
        },
        other => unreachable!("Expected Secrets validation error, got: {:?}", other),
    }
}

// Maps secret validation errors to load environment set secrets errors.
///
/// # Arguments
/// * `err` - The input validation error to map
///
/// # Panics
/// Panics if the error is not a secret validation error or is an unexpected secret error type.
pub fn map_secret_to_load_set_secrets_error(
    err: &InputValidationError,
) -> LoadEnvironmentInputValidationError {
    match err {
        InputValidationError::Secrets(secret_err) => match secret_err {
            SecretsInputValidationError::NameFormat(secret_names) => {
                LoadEnvironmentInputValidationError::SetSecretNameFormat(secret_names.clone())
            }
            SecretsInputValidationError::NameTooShort(secret_names) => {
                LoadEnvironmentInputValidationError::SetSecretNameTooShort(secret_names.clone())
            }
            SecretsInputValidationError::NameTooLong(secret_names) => {
                LoadEnvironmentInputValidationError::SetSecretNameTooLong(secret_names.clone())
            }
            other => unreachable!("Unexpected secret validation error: {:?}", other),
        },
        other => unreachable!("Expected Secrets validation error, got: {:?}", other),
    }
}
