use colored_json::to_colored_json_auto;
use core::fmt;
use owo_colors::OwoColorize;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum InputValidationError {
    CmdArgs(CmdArgInputValidationError),
    Projects(ProjectInputValidationError),
    Secrets(SecretsInputValidationError),
    Environments(EnvironmentsInputValidationError),
    YamlConfigFile(YamlEnvConfigError),
    Run(RunInputValidationError),
    LoadEnvironment(LoadEnvironmentInputValidationError),
    PushPullEnvironment(PushPullInputValidationError),
    Webhook(WebhookInputValidationError),
}

#[derive(Debug, Serialize)]
pub enum CmdArgInputValidationError {
    MissingProject,
    DuplicateProject,
    MissingEnvironment,
    DuplicateEnvironment,
    MissingProjectEnvironment,
}

#[derive(Debug, Serialize)]
pub enum ProjectInputValidationError {
    NameTooShort { is_root: bool },
    NameTooLong { is_root: bool },
    NameFormat { is_root: bool },

    InvalidIdentifierFormat { is_root: bool },
    NameUsingIdFormat,

    SearchTooShort,
    SearchFormat,
    InvalidLimit,
    InvalidPage,

    // update
    NoUpdateFlags,
    NewNameFormat,
    NewNameTooShort,
    NewNameTooLong,
    NewNameEqualsOriginal,
}

#[derive(Debug, Serialize)]
pub enum WebhookInputValidationError {
    // update
    NoUpdateFlags,
    InvalidLimit,
    InvalidId,
    InvalidUrl,
    DescriptionTooLong,
    InvalidPage,
}

// TODO: key length (min = 2 ???)
#[derive(Debug, Serialize)]
pub enum SecretsInputValidationError {
    NoNames,
    NamesFormat(Vec<String>),
    NamesTooShort(Vec<String>),
    NamesTooLong(Vec<String>),
    DuplicateNames(Vec<String>),
    DuplicateNewNames(Vec<String>),
    SelfReferences(Vec<String>),
    ReadFile(String),
    CommentsTooLong(Vec<String>),
    CommentTooLong,
    // names vec
    ValuesTooLong(Vec<String>),
    // for search command
    SearchBothNameAndValue,
    SearchMissingNameOrValue,
    SearchValueTooLong,
    SearchValueEmpty,

    SearchTooShort,
    SearchFormat,
    // update
    // SameNewKey,
    NoData,
    NewNamesFormat(Vec<String>),
    MissingPropertiesToUpdate(Vec<String>),
    NewNameSameAsName(Vec<String>),
}

// TODO: check if is used as value (env cmd) or as arg (secrets cmd)
#[derive(Debug, Serialize)]
pub enum EnvironmentsInputValidationError {
    NameTooShort { is_root: bool },
    NameTooLong { is_root: bool },
    NameFormat { is_root: bool },

    InvalidIdentifierFormat { is_root: bool },
    NameUsingIdFormat,

    SearchTooShort,
    SearchFormat,

    // update
    NewNameEqualsOriginal,
    NoUpdateFlags,
    NewNameFormat,
    NewNameTooShort,
    NewNameTooLong,
    //
    SelfComparison,
}

// geenral for stashbase.yaml file, shared between load, push and pull commands
#[derive(Debug, Serialize)]
pub enum YamlEnvConfigError {
    FileNotFound { custom_path: bool },
    FailedToRead { custom_path: bool, message: String },
    NoEntries,
}

#[derive(Debug, Serialize)]
pub enum LoadEnvironmentInputValidationError {
    FileArgWithInline,
    MissingProjectArg,
    MissingEnvArg,
    UseOfBothExcludeAndOnly,
    OnlySecretNamesFormat(Vec<String>),
    OnlySecretNamesTooShort(Vec<String>),
    OnlySecretNamesTooLong(Vec<String>),
    ExcludeSecretNamesFormat(Vec<String>),
    ExcludeSecretNamesTooShort(Vec<String>),
    ExcludeSecretNamesTooLong(Vec<String>),
    SetSecretNameValueSeparator,
    SetSecretNamesFormat(Vec<String>),
    SetSecretNamesTooShort(Vec<String>),
    SetSecretNamesTooLong(Vec<String>),
}

#[derive(Debug, Serialize)]
pub enum PushPullInputValidationError {
    NoFileSpecified { is_push: bool },
    // other errors same as from LoadEnvironment
}

#[derive(Debug, Serialize)]
pub enum RunInputValidationError {
    NoCmdProvided,
    NoSecretsToFetch,
}

impl fmt::Display for CmdArgInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: &str;

        match self {
            CmdArgInputValidationError::MissingProject => {
                msg = "Project not specified.";
                hint = "Use '-p/--project' argument to specify the project.";
            }
            CmdArgInputValidationError::DuplicateProject => {
                msg = "Project specified multiple times.";
                hint = "Use '-p/--project' argument only once.";
            }
            CmdArgInputValidationError::MissingEnvironment => {
                msg = "Environment not specified.";
                hint = "Use '-e/--environment' argument to specify the environment.";
            }
            CmdArgInputValidationError::DuplicateEnvironment => {
                msg = "Environment specified multiple times.";
                hint = "Use '-e/--environment' argument only once.";
            }
            CmdArgInputValidationError::MissingProjectEnvironment => {
                msg = "Project and environment not specified.";
                hint = "Use '-p/--project' and '-e/--environment' arguments.";
            }
        }

        writeln!(f, "{}", format!("  Message: {}", msg))?;
        write!(f, "{}", format!("  Hint: {}", hint))?;

        Ok(())
    }
}

impl fmt::Display for ProjectInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            ProjectInputValidationError::NameTooShort { is_root } => {
                if *is_root {
                    msg = "Argument name is too short.";
                    hint = Some("Minimum is 2 characters.");
                } else {
                    msg = "Project argument is too short.";
                    hint = Some("Minimum is 2 characters.");
                }
            }

            ProjectInputValidationError::NameTooLong { is_root } => {
                if *is_root {
                    msg = "Argument name is too long.";
                    hint = Some("Maximum is 40 characters.");
                } else {
                    msg = "Project argument is too long.";
                    hint = Some("Maximum is 40 characters.");
                }
            }
            ProjectInputValidationError::NameFormat { is_root } => {
                if *is_root {
                    msg = "Argument name is invalid.";
                    hint = Some("Name can contain only alphanumeric characters, hyphens or underscores (no spaces).");
                } else {
                    msg = "Argument project is invalid.";
                    hint = Some("Project name can contain only alphanumeric characters, hyphens or underscores.");
                }
            }
            ProjectInputValidationError::NoUpdateFlags => {
                msg = "No update option specified.";
                hint = Some("Use one of: -n (--name), -d (--description).");
            }
            ProjectInputValidationError::NewNameFormat => {
                msg = "Name option value is invalid.";
                hint = Some("Name can contain only alphanumeric characters, hyphens or underscores (no spaces).");
            }
            ProjectInputValidationError::NewNameTooShort => {
                msg = "Name option value is too short.";
                hint = Some("Minimum is 2 characters.");
            }
            ProjectInputValidationError::NewNameEqualsOriginal => {
                msg = "New name equals to original name.";
                hint = Some("Use different new name.");
            }
            ProjectInputValidationError::SearchTooShort => {
                msg = "Argument search is too short.";
                hint = Some("Minimum is 2 characters.");
            }
            ProjectInputValidationError::SearchFormat => {
                msg = "Argument search is invalid.";
                hint = Some(
                    "Search can contain only alphanumeric characters, hyphens or underscores.",
                );
            }
            ProjectInputValidationError::InvalidIdentifierFormat { is_root } => {
                if *is_root {
                    let  hint_str = "The name or id must be alphanumeric, name may include underscores (_) and hyphens(-) and must be between 2 to 40 characters long. Id must start with the prefix 'proj_' followed by 22 alphanumeric characters.";

                    msg = "Argument name or id is invalid.";
                    hint = Some(&hint_str);
                } else {
                    let  hint_str = "The project name or id must be alphanumeric, name may include underscores (_) and hyphens(-) and must be between 2 to 40 characters long. Id must start with the prefix 'proj_' followed by 22 alphanumeric characters.";

                    msg = "Argument project is invalid.";
                    hint = Some(&hint_str);
                }
            }
            ProjectInputValidationError::NameUsingIdFormat => {
                let hint_str = "Ensure the name is in a valid format: alphanumeric, may include underscores (_) and hyphens (-), without the prefix 'proj_' followed by 22 alphanumeric characters, min 2 max 40 characters.";

                msg = "Name is using id format.";
                hint = Some(&hint_str);
            }
            ProjectInputValidationError::NewNameTooLong => {
                msg = "Name option value is too long.";
                hint = Some("Maximum is 40 characters.");
            }
            ProjectInputValidationError::InvalidLimit => {
                msg = "Limit option value is invalid.";
                hint = Some("Limit can range from 2 to 30.");
            }
            ProjectInputValidationError::InvalidPage => {
                msg = "Page option value is invalid.";
                hint = Some("Page can range from 1 to 1000.");
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg))?;
            write!(f, "{}", format!("  Hint: {}", hint))?;
        } else {
            writeln!(f, "{}", format!("  Message: {}", msg))?;
        }

        Ok(())
    }
}

impl fmt::Display for SecretsInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;
        let mut secrets_names: Option<&Vec<String>> = None;

        match self {
            SecretsInputValidationError::NamesFormat(names) => {
                msg = "Invalid secret names.";
                hint = Some(
                    "Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed",
                );
                secrets_names = Some(names);
            }
            SecretsInputValidationError::NamesTooShort(names) => {
                msg = "Secret names are too short.";
                hint = Some("Minimum length for secret name is 2 characters.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::NamesTooLong(names) => {
                msg = "Secret names are too long.";
                hint = Some("Maximum length for secret name is 255 characters.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::CommentsTooLong(names) => {
                msg = "Secret comments are too long.";
                hint = Some("Maximum length for comment is 512 characters (after formatting).");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::CommentTooLong => {
                msg = "Secret comment is too long.";
                hint = Some("Maximum length for comment is 512 characters (after formatting).");
            }
            SecretsInputValidationError::SearchFormat => {
                msg = "Argument search is invalid.";
                hint = Some(
                    "Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed.",
                );
            }
            SecretsInputValidationError::SearchTooShort => {
                msg = "Argument search is too short.";
                hint = Some("Minimum is 2 characters.");
            }
            SecretsInputValidationError::NoNames => {
                msg = "No secrets names specified.";
                hint = Some("Separate names of secrets to return with spaces.");
            }
            SecretsInputValidationError::DuplicateNames(names) => {
                msg = "Found duplicate secret names.";
                hint = Some("Secret names cannot be used more than once.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::DuplicateNewNames(names) => {
                msg = "Found duplicate new names.";
                hint = Some("New names cannot be used more than once.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::SelfReferences(names) => {
                msg = "Found self-referencing secrets.";
                hint = Some("Secrets cannot reference themselves.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::ReadFile(error) => {
                let msg = "Error reading file.";

                writeln!(f, "{}", format!("  Message: {}", msg))?;
                write!(f, "{}", format!("  Details: {}", error))?;

                return Ok(());
            }
            SecretsInputValidationError::SearchBothNameAndValue => {
                msg = "Cannot provide both 'name' and 'value' options.";
                hint = Some("Provide only one of them.");
            }
            SecretsInputValidationError::SearchMissingNameOrValue => {
                msg = "No search criteria provided.";
                hint = Some("Provide either 'name' or 'value' option.");
            }
            SecretsInputValidationError::SearchValueTooLong => {
                msg = "Option 'value' is too long.";
                hint = Some("Maximum length is 1000 characters.");
            }
            SecretsInputValidationError::SearchValueEmpty => {
                msg = "Option 'value' is empty.";
                hint = Some("Provide non-empty string value.");
            }
            SecretsInputValidationError::ValuesTooLong(secret_names) => {
                msg = "Secret values are too long.";
                hint = Some("Maximum length is 4096 characters.");
                secrets_names = Some(secret_names);
            }
            SecretsInputValidationError::NoData => {
                msg = "No data provided.";
                hint = Some("Provide valid secret data.");
            }
            SecretsInputValidationError::NewNamesFormat(names) => {
                msg = "Invalid new secret names.";
                hint = Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::MissingPropertiesToUpdate(names) => {
                msg = "Missing properties to update.";
                hint = Some("Provide valid secret data.");
                secrets_names = Some(names);
            }
            SecretsInputValidationError::NewNameSameAsName(names) => {
                msg = "New name equals to original name.";
                hint = Some("Use different new name.");
                secrets_names = Some(names);
            }
        }

        write!(f, "  Message: {}", msg)?;

        if let Some(hint) = hint {
            write!(f, "\n  Hint: {}", hint)?;
        }

        if let Some(secrets_names) = secrets_names {
            if !secrets_names.is_empty() {
                let formatted_secrets = secrets_names
                    .iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ");

                write!(f, "\n  Secrets: {}", formatted_secrets)?;
            }
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
                    msg = "Argument name is too short.";
                    hint = Some("Minimum is 2 characters.");
                } else {
                    msg = "Environment argument is too short.";
                    hint = Some("Minimum is 2 characters.");
                }
            }
            EnvironmentsInputValidationError::NameFormat { is_root } => {
                if *is_root {
                    msg = "Argument name is invalid.";
                    hint = Some(
                        "Name can contain only alphanumeric characters, underscores or hyphen separator (no spaces).",
                    );
                } else {
                    msg = "Argument environment is invalid.";
                    hint = Some("Environment name can contain only alphanumeric characters, underscores or hyphen separator.");
                }
            }
            EnvironmentsInputValidationError::NewNameEqualsOriginal => {
                msg = "Provided new name equals to original name.";
                hint = Some("Use different new name.");
            }
            EnvironmentsInputValidationError::NoUpdateFlags => {
                msg = "No update flag specified.";
                hint = Some("Use one of: -n (--name), -d (--description), -t (--type).");
            }
            EnvironmentsInputValidationError::NewNameFormat => {
                msg = "New name option value is invalid.";
                hint = Some("Name can contain only alphanumeric characters, underscores or hyphen separator (no spaces).");
            }
            EnvironmentsInputValidationError::NewNameTooShort => {
                msg = "New name option value is too short.";
                hint = Some("Minimum is 2 characters.");
            }
            EnvironmentsInputValidationError::SearchTooShort => {
                msg = "Argument search is too short.";
                hint = Some("Minimum is 2 characters.");
            }
            EnvironmentsInputValidationError::SearchFormat => {
                msg = "Argument search is invalid.";
                hint =
                    Some("Search can contain only alphanumeric characters, underscores or hyphen separator.");
            }
            EnvironmentsInputValidationError::InvalidIdentifierFormat { is_root } => {
                if *is_root {
                    let  hint_str = "The name or id must be alphanumeric, name may include underscores (_) and a signle hyphen (-) as as separator and must be between 2 to 40 characters long. Id must start with the prefix 'env_' followed by 22 alphanumeric characters.";

                    msg = "Argument name or id is invalid.";
                    hint = Some(&hint_str);
                } else {
                    let  hint_str = "The environment name or id must be alphanumeric, name may include underscores (_) and a signle hyphen (-) as as separator and must be between 2 to 40 characters long. Id must start with the prefix 'env_' followed by 22 alphanumeric characters.";

                    msg = "Argument environment is invalid.";
                    hint = Some(&hint_str);
                }
            }
            EnvironmentsInputValidationError::NameUsingIdFormat => {
                let hint_str = "Ensure the name is in a valid format: alphanumeric, may include underscores (_) and a signle hyphen (-) as as separator, without the prefix 'env_' followed by 22 alphanumeric characters, min 2 max 40 characters.";

                msg = "Name is using id format.";
                hint = Some(&hint_str);
            }
            EnvironmentsInputValidationError::NameTooLong { is_root } => {
                if *is_root {
                    msg = "Argument name is too long.";
                    hint = Some("Maximum is 40 characters.");
                } else {
                    msg = "Project argument is too long.";
                    hint = Some("Maximum is 40 characters.");
                }
            }
            EnvironmentsInputValidationError::NewNameTooLong => {
                msg = "New name option value is too long.";
                hint = Some("Maximum is 40 characters.");
            }
            EnvironmentsInputValidationError::SelfComparison => {
                msg = "Cannot compare an environment with itself.";
                hint = Some("Use different environment for comparison.");
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
            write!(f, "{}", format!("  Hint: {}", hint),)?;
        } else {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for LoadEnvironmentInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;
        let mut secrets_names: Option<&Vec<String>> = None;

        match self {
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly => {
                msg = "Use of both --exclude and --only flag.";
                hint = Some("Use only one of them.");
            }
            LoadEnvironmentInputValidationError::OnlySecretNamesFormat(names) => {
                msg = "Invalid only secret names.";
                hint = Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::OnlySecretNamesTooShort(names) => {
                msg = "Only argument secret names are too short.";
                hint = Some("Minimum length for secret name is 2 characters.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::OnlySecretNamesTooLong(names) => {
                msg = "Only argument secret names are too long.";
                hint = Some("Maximum length for secret name is 255 characters.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::ExcludeSecretNamesFormat(names) => {
                msg = "Invalid exclude secret names.";
                hint = Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::ExcludeSecretNamesTooShort(names) => {
                msg = "Exclude secret names are too short.";
                hint = Some("Minimum length for secret name is 2 characters.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::ExcludeSecretNamesTooLong(names) => {
                msg = "Exclude secret names are too long.";
                hint = Some("Maximum length for secret name is 255 characters.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::MissingProjectArg => {
                msg = "Missing project argument.";
                hint = Some("Use '-p' flag to specify the project.");
            }
            LoadEnvironmentInputValidationError::MissingEnvArg => {
                msg = "Missing environment argument.";
                hint = Some("Use '-e' flag to specify the environment.");
            }
            LoadEnvironmentInputValidationError::FileArgWithInline => {
                msg = "Cannot use '--file' flag and '-p' or '-e' flag at the same time.";
                hint = None;
            }
            LoadEnvironmentInputValidationError::SetSecretNameValueSeparator => {
                msg = "Invalid set argument.";
                hint = Some("Expected a name-value pair (separated by '=').");
            }
            LoadEnvironmentInputValidationError::SetSecretNamesFormat(names) => {
                msg = "Invalid set secret names.";
                hint = Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::SetSecretNamesTooShort(names) => {
                msg = "Set secret names are too short.";
                hint = Some("Minimum length for secret name is 2 characters.");
                secrets_names = Some(names);
            }
            LoadEnvironmentInputValidationError::SetSecretNamesTooLong(names) => {
                msg = "Set secret names are too long.";
                hint = Some("Maximum length for secret name is 255 characters.");
                secrets_names = Some(names);
            }
        }

        write!(f, "  Message: {}", msg)?;

        if let Some(hint) = hint {
            write!(f, "\n  Hint: {}", hint)?;
        }

        if let Some(secrets_names) = secrets_names {
            if !secrets_names.is_empty() {
                let formatted_secrets = secrets_names
                    .iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ");

                write!(f, "\n  Secrets: {}", formatted_secrets)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for PushPullInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            PushPullInputValidationError::NoFileSpecified { is_push } => match is_push {
                true => {
                    msg = "No file specified.";
                    hint =
                    Some("Add root property 'file' or push property 'file' to the config or use '--file' flag.");
                }
                false => {
                    msg = "No file specified.";
                    hint =
                    Some("Add root property 'file' or pull property 'file' to the config or use '--file' flag.");
                }
            },
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
            write!(f, "{}", format!("  Hint: {}", hint),)?;
        } else {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
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
                msg = "No update flag specified.";
                hint = Some("Use one of: -u (--url), -d (--description).");
            }
            WebhookInputValidationError::InvalidLimit => {
                msg = "Invalid '--limit' option value.";
                hint = Some("Limit can range from 2 to 30.");
            }
            WebhookInputValidationError::InvalidId => {
                let hint_str =
                    "Id must start with the prefix 'whk_' followed by 22 alphanumeric characters.";

                msg = "Invalid webhook id value.";
                hint = Some(&hint_str);
            }
            WebhookInputValidationError::InvalidUrl => {
                msg = "Invalid webhook url.";
                hint = Some("Must be valid url using https protocol.");
            }
            WebhookInputValidationError::DescriptionTooLong => {
                msg = "Description is too long.";
                hint = Some("Maximum is 200 characters.");
            }
            WebhookInputValidationError::InvalidPage => {
                msg = "Page option value is invalid.";
                hint = Some("Page can range from 1 to 1000.");
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
            write!(f, "{}", format!("  Hint: {}", hint),)?;
        } else {
            write!(f, "{}", format!("  Message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for RunInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            RunInputValidationError::NoCmdProvided => {
                msg = "No command provided.";
                hint = Some("Provide command you want to run.");
            }
            RunInputValidationError::NoSecretsToFetch => {
                msg = "No secrets to fetch.";
                hint = Some("'set' secrets option overrides all secrets from option 'only'.");
            }
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
            write!(f, "{}", format!("  Hint: {}", hint),)?;
        } else {
            write!(f, "{}", format!("  Message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for YamlEnvConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            YamlEnvConfigError::FileNotFound { custom_path } => match custom_path {
                true => {
                    msg = "No config file found.";
                    hint = Some("Make sure the specified file exists.");
                }
                false => {
                    msg = "No 'stashbase.yaml' file found.";
                    hint = Some("Create file or use '-p' and '-e' flags.");
                }
            },
            YamlEnvConfigError::NoEntries => {
                msg = "No entries found in 'stashbase.yaml'.";
                hint = Some("Add entries to the file or use '-p' and '-e' flags.");
            }
            YamlEnvConfigError::FailedToRead {
                custom_path,
                message,
            } => match custom_path {
                true => {
                    msg = "Failed to read the specified config file.";
                    hint = Some(message);
                }
                false => {
                    msg = "Failed to read 'stashbase.yaml' file.";
                    hint = Some(message);
                }
            },
        }

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg))?;
            write!(f, "{}", format!("  Hint: {}", hint))?;
        } else {
            write!(f, "{}", format!("  Message: {}", msg))?;
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
            InputValidationError::LoadEnvironment(inner) => write!(f, "{}", inner),
            InputValidationError::PushPullEnvironment(inner) => write!(f, "{}", inner),
            InputValidationError::Webhook(inner) => write!(f, "{}", inner),
            InputValidationError::CmdArgs(inner) => write!(f, "{}", inner),
            InputValidationError::Run(inner) => write!(f, "{}", inner),
            InputValidationError::YamlConfigFile(inner) => write!(f, "{}", inner),
        }
    }
}

impl InputValidationError {
    pub fn format_error_output(self, json_format: bool) -> Result<String, serde_json::Error> {
        if json_format {
            let json_err = self.to_colored_json()?;
            Ok(json_err)
        } else {
            Ok(self.to_string())
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        #[derive(serde::Serialize)]
        struct ErrorWrapper<'a> {
            #[serde(rename = "error")]
            error: ErrorData<'a>,
        }

        #[derive(serde::Serialize)]
        struct ErrorData<'a> {
            #[serde(flatten)]
            data: &'a InputValidationError,
            // #[serde(rename = "type")]
            // error_type: &'static str,
        }

        let wrapper = ErrorWrapper {
            error: ErrorData { data: self },
        };
        serde_json::to_value(&wrapper)
    }

    pub fn to_colored_json(&self) -> Result<String, serde_json::Error> {
        let json_value = self.to_json_value()?;
        let json_str = to_colored_json_auto(&json_value)?;

        Ok(json_str)
    }
}
