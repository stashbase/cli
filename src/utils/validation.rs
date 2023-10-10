use anyhow::{bail, Result};
use regex::Regex;

use crate::models::validation::{
    EnvironmentsInputValidationError, InputValidationError, ProjectInputValidationError,
    SecretsInputValidationError,
};

pub fn validate_project_name(value: &str, is_new_name: bool) -> Result<()> {
    if value.len() < 2 {
        if is_new_name == false {
            bail!(InputValidationError::Projects(
                ProjectInputValidationError::NameTooShort
            ))
        } else {
            bail!(InputValidationError::Projects(
                ProjectInputValidationError::NewNameTooShort
            ));
        }
    }
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if !regex.is_match(value) {
        if is_new_name == false {
            bail!(InputValidationError::Projects(
                ProjectInputValidationError::NameFormat
            ))
        } else {
            bail!(InputValidationError::Projects(
                ProjectInputValidationError::NewNameFormat
            ))
        }
    }

    Ok(())
}

// TODO: validate env name

// name of secret
pub fn validate_secret_key(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    if !regex.is_match(value) {
        bail!(InputValidationError::Secrets(
            SecretsInputValidationError::KeyFormat { multiple: false }
        ));
    } else {
        Ok(())
    }
}

pub fn validate_secret_keys(values: &Vec<String>) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    let invalid = values.into_iter().find(|v| !regex.is_match(*v));

    if invalid.is_some() {
        let multiple = values.len() > 1;

        bail!(InputValidationError::Secrets(
            SecretsInputValidationError::KeyFormat { multiple }
        ));
    } else {
        Ok(())
    }
}

pub fn validate_secret_key_new_key(values: &Vec<(String, String)>) -> Result<()> {
    let regex = Regex::new(r"^[A-Z0-9_]+$").unwrap();

    let invalid = values
        .into_iter()
        .find(|k| !regex.is_match(&k.0) || !regex.is_match(&k.1));

    if invalid.is_some() {
        bail!(InputValidationError::Secrets(
            SecretsInputValidationError::KeyFormat { multiple: true }
        ));
    } else {
        Ok(())
    }
}

pub fn validate_environment_name(value: &str) -> Result<()> {
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if value.len() < 2 {
        let err =
            InputValidationError::Environments(EnvironmentsInputValidationError::NameTooShort);
        bail!("{}", err)
    } else {
        if !regex.is_match(value) {
            let err =
                InputValidationError::Environments(EnvironmentsInputValidationError::NameFormat);
            bail!("{}", err)
        } else {
            Ok(())
        }
    }
}
