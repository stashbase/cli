use anyhow::{bail, Result};
use regex::Regex;

use crate::models::validation::{InputValidationError, ProjectInputValidationError};

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
