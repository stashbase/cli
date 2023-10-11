use anyhow::{bail, Result};
use regex::Regex;

use crate::models::validation::{
    EnvironmentsInputValidationError, InputValidationError, ProjectInputValidationError,
    SecretsInputValidationError,
};

pub fn validate_project_name(value: &str, is_new_name: bool, is_root: bool) -> Result<()> {
    if value.len() < 2 {
        let err = if is_new_name {
            InputValidationError::Projects(ProjectInputValidationError::NewNameTooShort)
        } else {
            InputValidationError::Projects(ProjectInputValidationError::NameTooShort { is_root })
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

// name of secret
pub fn validate_secret_key(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    if !regex.is_match(value) {
        let err = InputValidationError::Secrets(SecretsInputValidationError::KeyFormat {
            multiple: false,
        });

        bail!(err)
    }

    Ok(())
}

pub fn validate_secret_keys(values: &Vec<String>) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    let invalid = values.into_iter().find(|v| !regex.is_match(*v));

    if invalid.is_some() {
        let multiple = values.len() > 1;
        let err =
            InputValidationError::Secrets(SecretsInputValidationError::KeyFormat { multiple });

        bail!(err)
    }

    Ok(())
}

pub fn validate_secret_key_new_key(values: &Vec<(String, String)>) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    let invalid = values
        .into_iter()
        .find(|k| !regex.is_match(&k.0) || !regex.is_match(&k.1));

    if invalid.is_some() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::KeyFormat {
            multiple: true,
        });

        bail!(err)
    }

    Ok(())
}

pub fn validate_environment_name(value: &str, is_new_name: bool, is_root: bool) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if value.len() < 2 {
        let err = if is_new_name == false {
            InputValidationError::Environments(EnvironmentsInputValidationError::NameTooShort {
                is_root,
            })
        } else {
            InputValidationError::Environments(EnvironmentsInputValidationError::NewNameTooShort)
        };

        bail!(err)
    } else {
        if !regex.is_match(value) {
            let err = if is_new_name == false {
                InputValidationError::Environments(EnvironmentsInputValidationError::NameFormat {
                    is_root,
                })
            } else {
                InputValidationError::Environments(EnvironmentsInputValidationError::NewNameFormat)
            };

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

//
pub fn validate_env_search(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if value.len() < 2 {
        let err =
            InputValidationError::Environments(EnvironmentsInputValidationError::SearchTooShort);

        bail!(err)
    } else {
        if !regex.is_match(value) {
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
