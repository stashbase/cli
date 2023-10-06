// #[derive(Debug)]
// pub struct InputValidationError {
//     pub message: String,
//     pub hint: Option<String>,
// }
//
// #[derive(Debug)]
// pub enum ProjectInputValidationError {
//     NameTooShort,
// }
//

use core::fmt;

use owo_colors::OwoColorize;

#[derive(Debug)]
pub enum InputValidationError {
    Projects(ProjectInputValidationError),
}

#[derive(Debug)]
pub enum ProjectInputValidationError {
    NameTooShort,
    NameFormat,

    // update
    NoUpdateFlags,
    NewNameFormat,
    NewNameTooShort,
    SameNewName,
}

impl fmt::Display for ProjectInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            ProjectInputValidationError::NameTooShort => {
                msg = "argument name is too short";
                hint = Some("minimum is 2 characters");
            }
            ProjectInputValidationError::NameFormat => {
                msg = "argument name is invalid";
                hint = Some("name can contain only alphanumeric characters, hyphens or underscores (no spaces)");
            }
            ProjectInputValidationError::NoUpdateFlags => {
                msg = "no update flag specified";
                hint = Some("use one of: -n (--name), -d (--description)");
            }
            ProjectInputValidationError::NewNameFormat => {
                msg = "name option value is invalid";
                hint = Some("name can contain only alphanumeric characters, hyphens or underscores (no spaces)");
            }
            ProjectInputValidationError::NewNameTooShort => {
                msg = "name option value is too short";
                hint = Some("minimum is 2 characters");
            }
            ProjectInputValidationError::SameNewName => {
                msg = "name option value is equals to name";
                hint = Some("use different name option value");
            }
        }

        //

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
            write!(f, "{}", format!("- hint: {}", hint),)?;
        } else {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for InputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", "Input error".red().bold())?;
        match self {
            InputValidationError::Projects(inner) => write!(f, "{}", inner),
        }
    }
}
