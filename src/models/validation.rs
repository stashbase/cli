use core::fmt;
use owo_colors::OwoColorize;

#[derive(Debug)]
pub enum InputValidationError {
    Projects(ProjectInputValidationError),
    Secrets(SecretsInputValidationError),
    Environments(EnvironmentsInputValidationError),
}

#[derive(Debug)]
pub enum ProjectInputValidationError {
    NameTooShort { is_root: bool },
    NameFormat { is_root: bool },

    SearchTooShort,
    SearchFormat,

    // update
    NoUpdateFlags,
    NewNameFormat,
    NewNameTooShort,
    SameNewName,
}

// TODO: key length (min = 2 ???)
#[derive(Debug)]
pub enum SecretsInputValidationError {
    KeyFormat { multiple: bool },
    // update
    // SameNewKey,
}

// TODO: check if is used as value (env cmd) or as arg (secrets cmd)
#[derive(Debug)]
pub enum EnvironmentsInputValidationError {
    NameTooShort { is_root: bool },
    NameFormat { is_root: bool },

    SearchTooShort,
    SearchFormat,

    // update
    SameNewName,
    NoUpdateFlags,
    NewNameFormat,
    NewNameTooShort,
}

impl fmt::Display for ProjectInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            ProjectInputValidationError::NameTooShort { is_root } => {
                if *is_root {
                    msg = "argument name is too short";
                    hint = Some("minimum is 2 characters");
                } else {
                    msg = "project argument is too short";
                    hint = Some("minimum is 2 characters");
                }
            }
            ProjectInputValidationError::NameFormat { is_root } => {
                if *is_root {
                    msg = "argument name is invalid";
                    hint = Some("name can contain only alphanumeric characters, hyphens or underscores (no spaces)");
                } else {
                    msg = "argument project is invalid";
                    hint = Some("project name can contain only alphanumeric characters, hyphens or underscores");
                }
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
            ProjectInputValidationError::SearchTooShort => {
                msg = "argument search is too short";
                hint = Some("minimum is 2 characters");
            }
            ProjectInputValidationError::SearchFormat => {
                msg = "argument search is invalid";
                hint =
                    Some("search can contain only alphanumeric characters, hyphens or underscores");
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
            write!(f, "{}", format!("- hint: {}", hint),)?;
        } else {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for SecretsInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            SecretsInputValidationError::KeyFormat { multiple } => {
                let message = match multiple {
                    true => "invalid secret keys",
                    false => "invalid secret key",
                };
                msg = message;
                hint = Some(
                    "secret key can contain only uppercase alphanumeric characters and underscores",
                );
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
            write!(f, "{}", format!("- hint: {}", hint),)?;
        } else {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for EnvironmentsInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            EnvironmentsInputValidationError::NameTooShort { is_root } => {
                if *is_root {
                    msg = "argument name is too short";
                    hint = Some("minimum is 2 characters");
                } else {
                    msg = "environment argument is too short";
                    hint = Some("minimum is 2 characters");
                }
            }
            EnvironmentsInputValidationError::NameFormat { is_root } => {
                if *is_root {
                    msg = "argument name is invalid";
                    hint = Some(
                        "name can contain only alphanumeric characters, hyphens or underscores",
                    );
                } else {
                    msg = "argument environment is invalid";
                    hint = Some("environment name can contain only alphanumeric characters, hyphens or underscores");
                }
            }
            EnvironmentsInputValidationError::SameNewName => {
                msg = "new name option value is equals to name";
                hint = Some("use different name option value");
            }
            EnvironmentsInputValidationError::NoUpdateFlags => {
                msg = "no update flag specified";
                hint = Some("use one of: -n (--name), -d (--description)");
            }
            EnvironmentsInputValidationError::NewNameFormat => {
                msg = "new name option value is invalid";
                hint = Some("name can contain only alphanumeric characters, hyphens or underscores (no spaces)");
            }
            EnvironmentsInputValidationError::NewNameTooShort => {
                msg = "name option value is too short";
                hint = Some("minimum is 2 characters");
            }
            EnvironmentsInputValidationError::SearchTooShort => {
                msg = "argument search is too short";
                hint = Some("minimum is 2 characters");
            }
            EnvironmentsInputValidationError::SearchFormat => {
                msg = "argument search is invalid";
                hint =
                    Some("search can contain only alphanumeric characters, hyphens or underscores");
            }
        }

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
            InputValidationError::Secrets(inner) => write!(f, "{}", inner),
            InputValidationError::Environments(inner) => write!(f, "{}", inner),
        }
    }
}
