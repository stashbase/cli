use anyhow::{bail, Result};
use regex::Regex;

use crate::models::validation::{
    EnvironmentsInputValidationError, InputValidationError, ProjectInputValidationError,
    SecretsInputValidationError,
};

pub fn validate_project_name(value: &str, is_new_name: bool) -> Result<()> {
    if value.len() < 2 {
        let err = if is_new_name {
            InputValidationError::Projects(ProjectInputValidationError::NewNameTooShort)
        } else {
            InputValidationError::Projects(ProjectInputValidationError::NameTooShort)
        };

        bail!(err)
    }

    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if !regex.is_match(value) {
        let err = if is_new_name {
            InputValidationError::Projects(ProjectInputValidationError::NameFormat)
        } else {
            InputValidationError::Projects(ProjectInputValidationError::NewNameFormat)
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

pub fn validate_environment_name(value: &str, is_new_name: bool) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if value.len() < 2 {
        let err = if is_new_name == false {
            InputValidationError::Environments(EnvironmentsInputValidationError::NameTooShort)
        } else {
            InputValidationError::Environments(EnvironmentsInputValidationError::NewNameTooShort)
        };

        bail!(err)
    } else {
        if !regex.is_match(value) {
            let err = if is_new_name == false {
                InputValidationError::Environments(EnvironmentsInputValidationError::NameFormat)
            } else {
                InputValidationError::Environments(EnvironmentsInputValidationError::NewNameFormat)
            };

            bail!(err)
        }
    }

    Ok(())
}

pub fn validate_project_environment(project: &str, environment: &str) -> Result<()> {
    let project_name_is_valid = validate_project_name(project, false);

    if let Err(err) = project_name_is_valid {
        bail!(err);
    }

    // validate env
    let env_name_is_valid = validate_environment_name(environment, false);

    if let Err(err) = env_name_is_valid {
        bail!(err);
    }

    Ok(())
}
