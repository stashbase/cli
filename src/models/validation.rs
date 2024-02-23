use core::fmt;
use owo_colors::OwoColorize;

#[derive(Debug)]
pub enum InputValidationError {
    Projects(ProjectInputValidationError),
    Secrets(SecretsInputValidationError),
    Environments(EnvironmentsInputValidationError),
    EnvChangelog(EnvChangelogInputValidationError),
    LoadEnvironment(LoadEnvironmentInputValidationError),
    PullEnvironment(PullEnvironmentInputValidationError),
    Webhook(WebhookInputValidationError),
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

#[derive(Debug)]
pub enum WebhookInputValidationError {
    // update
    NoUpdateFlags,
    InvalidPerPage,

    InvalidId,
}

// TODO: key length (min = 2 ???)
#[derive(Debug)]
pub enum SecretsInputValidationError {
    KeyFormat { multiple: bool },

    SearchTooShort,
    SearchFormat,
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

#[derive(Debug)]
pub enum EnvChangelogInputValidationError {
    InvalidIdFormat,
    InvalidIdLength,
}

#[derive(Debug)]
pub enum LoadEnvironmentInputValidationError {
    NoConfigFile { custom_path: bool },
    NoConfigFileEntries,
    FileArgWithInline,
    MissingProjectArg,
    MissingEnvArg,
    UseOfBothExcludeAndOnly,
    OnlyKeyFormat,
    ExcludeKeyFormat,
    SetKeyValueSeparator,
    SetKeyValueFormat,
}

#[derive(Debug)]
pub enum PullEnvironmentInputValidationError {
    NoConfigFile { custom_path: bool },
    NoConfigFileEntries,
    // other errors same as from LoadEnvironment
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
            writeln!(f, "{}", format!("- message: {}", msg))?;
            write!(f, "{}", format!("- hint: {}", hint))?;
        } else {
            writeln!(f, "{}", format!("- message: {}", msg))?;
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
            SecretsInputValidationError::SearchFormat => {
                msg = "argument search is invalid";
                hint = Some(
                    "secret key can contain only uppercase alphanumeric characters and underscores",
                );
            }
            SecretsInputValidationError::SearchTooShort => {
                msg = "argument search is too short";
                hint = Some("minimum is 2 characters");
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

impl fmt::Display for EnvChangelogInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            EnvChangelogInputValidationError::InvalidIdFormat => {
                msg = "invalid id";
                hint = Some("is must be alphanumeric");
            }
            EnvChangelogInputValidationError::InvalidIdLength => {
                msg = "invalid id";
                hint = Some("id must be 22 characters long");
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

impl fmt::Display for LoadEnvironmentInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly => {
                msg = "use of both --exclude and --only flag";
                hint = Some("use only one of them");
            }
            LoadEnvironmentInputValidationError::OnlyKeyFormat => {
                msg = "invalid only argument";
                hint = Some("accepts only uppercase alphanumeric characters and underscores");
            }
            LoadEnvironmentInputValidationError::ExcludeKeyFormat => {
                msg = "invalid exclude argument";
                hint = Some("accepts only uppercase alphanumeric characters and underscores");
            }
            LoadEnvironmentInputValidationError::NoConfigFile { custom_path } => {
                match custom_path {
                    true => {
                        msg = "no config file found";
                        hint = Some("make sure the file exists");
                    }
                    false => {
                        msg = "no 'env-ease.yaml' config file found";
                        hint = Some("create file or use '-p' and '-e' flags");
                    }
                };
            }
            LoadEnvironmentInputValidationError::NoConfigFileEntries => {
                msg = "no entries found in 'env-ease.yaml'";
                hint = Some("add entries to the file or use '-p' and '-e' flags");
            }
            LoadEnvironmentInputValidationError::MissingProjectArg => {
                msg = "missing project argument";
                hint = Some("use '-p' flag to specify the project");
            }
            LoadEnvironmentInputValidationError::MissingEnvArg => {
                msg = "missing environment argument";
                hint = Some("use '-e' flag to specify the environment");
            }
            LoadEnvironmentInputValidationError::FileArgWithInline => {
                msg = "cannot use '--file' flag and '-p' or '-e' flag at the same time";
                hint = None;
            }
            LoadEnvironmentInputValidationError::SetKeyValueSeparator => {
                msg = "invalid set argument";
                hint = Some("expected a key-value pair (separated by '=')");
            }
            LoadEnvironmentInputValidationError::SetKeyValueFormat => {
                msg = "invalid set argument";
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

impl fmt::Display for PullEnvironmentInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            PullEnvironmentInputValidationError::NoConfigFile { custom_path } => {
                match custom_path {
                    true => {
                        msg = "no config file found";
                        hint = Some("make sure the file exists");
                    }
                    false => {
                        msg = "no 'stashbase.yaml' config file found";
                        hint = Some("ceate the file or provide file path with '-c' flag");
                    }
                };
            }
            PullEnvironmentInputValidationError::NoConfigFileEntries => {
                msg = "no entries found in 'onestash.yaml'";
                hint = Some("add entries to the file or use '-p' and '-e' flags");
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

impl fmt::Display for WebhookInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            WebhookInputValidationError::NoUpdateFlags => {
                msg = "no update flag specified";
                hint = Some("use one of: -u (--url), -d (--description)");
            }
            WebhookInputValidationError::InvalidPerPage => {
                msg = "invalid '--per-page' option value";
                hint = Some("value can be 5, 10, 15 or 20");
            }
            WebhookInputValidationError::InvalidId => {
                msg = "invalid webhook id value";
                hint = None;
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("- message: {}", msg),)?;
            write!(f, "{}", format!("- hint: {}", hint),)?;
        } else {
            write!(f, "{}", format!("- message: {}", msg),)?;
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
            InputValidationError::EnvChangelog(inner) => write!(f, "{}", inner),
            InputValidationError::LoadEnvironment(inner) => write!(f, "{}", inner),
            InputValidationError::PullEnvironment(inner) => write!(f, "{}", inner),
            InputValidationError::Webhook(inner) => write!(f, "{}", inner),
        }
    }
}
