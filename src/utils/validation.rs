use anyhow::{bail, Result};
use regex::Regex;

use crate::models::validation::{InputValidationError, ProjectInputValidationError};

pub fn validate_project_name(value: &str) -> Result<()> {
    if value.len() < 2 {
        bail!(InputValidationError::Projects(
            ProjectInputValidationError::NameTooShort
        ));
    }
    let regex = Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap();

    if !regex.is_match(value) {
        bail!(InputValidationError::Projects(
            ProjectInputValidationError::NameFormat
        ))
    }

    Ok(())
}
