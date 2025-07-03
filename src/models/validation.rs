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
    Scan(ScanInputValidationError),
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
    NoSecretsToCreate,
    NoSecretsToDelete,
    NoSecretsToSet,
    NoUpdatesProvided,
    NamesFormat(Vec<String>),
    NamesTooShort(Vec<String>),
    NamesTooLong(Vec<String>),
    DuplicateNames(Vec<String>),
    DuplicateNewNames(Vec<String>),
    SelfReferences(Vec<String>),
    ReadFile(String),
    CommentsTooLong(Vec<String>),
    CommentTooLong,
    NameValueSeparator,
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
pub enum ScanInputValidationError {
    BaselineFileNotFound { path: String },
    BaselineFileRead { path: String, message: String },
    BaselineFileParse { path: String, message: String },
    GitRepositoryNotFound,
    GitRepositoryAccess { message: String },
    GitIndexAccess { message: String },
    GitHeadAccess { message: String },
    GitBranchAccess { message: String },
    GitCommitAccess { message: String },
    GitTreeAccess { message: String },
    GitDiffGeneration { message: String },
    GitDiffProcessing { message: String },
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
        let (msg, hint) = self.message_and_hint();

        writeln!(f, "{}", format!("  Message: {}", msg))?;
        write!(f, "{}", format!("  Hint: {}", hint))?;

        Ok(())
    }
}

impl fmt::Display for ProjectInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (msg, hint) = self.message_and_hint();

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
        let (msg, hint, secrets_names) = self.message_and_hint_and_secrets();

        write!(f, "  Message: {}", msg)?;

        if let Some(hint) = hint {
            write!(f, "\n  Hint: {}", hint)?;
        }

        if !secrets_names.is_empty() {
            let formatted_secrets = secrets_names
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ");

            write!(f, "\n  Secrets: {}", formatted_secrets)?;
        }

        Ok(())
    }
}

impl fmt::Display for EnvironmentsInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (msg, hint) = self.message_and_hint();

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
        let (msg, hint, secrets_names) = self.message_and_hint_and_secrets();

        write!(f, "  Message: {}", msg)?;

        if let Some(hint) = hint {
            write!(f, "\n  Hint: {}", hint)?;
        }

            if !secrets_names.is_empty() {
                let formatted_secrets = secrets_names
                    .iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ");

                write!(f, "\n  Secrets: {}", formatted_secrets)?;
        }

        Ok(())
    }
}

impl fmt::Display for PushPullInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (msg, hint) = self.message_and_hint();


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
        let (msg, hint) = self.message_and_hint();


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
        let (msg, hint) = self.message_and_hint();

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg),)?;
            write!(f, "{}", format!("  Hint: {}", hint),)?;
        } else {
            write!(f, "{}", format!("  Message: {}", msg),)?;
        }

        Ok(())
    }
}

impl fmt::Display for ScanInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (msg, hint) = self.message_and_hint();

        if let Some(hint) = hint {
            writeln!(f, "{}", format!("  Message: {}", msg))?;
            write!(f, "{}", format!("  Hint: {}", hint))?;
        } else {
            write!(f, "{}", format!("  Message: {}", msg))?;
        }

        Ok(())
    }
}

impl fmt::Display for YamlEnvConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (msg, hint) = self.message_and_hint();


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
            InputValidationError::Scan(inner) => write!(f, "{}", inner),
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
            data: &'a MessageHint,
            #[serde(rename = "type")]
            error_type: &'static str,
        }

        let wrapper = ErrorWrapper {
            error: ErrorData { data: &self.to_struct(), error_type: "input_validation_error" },
        };
        serde_json::to_value(&wrapper)
    }

    pub fn to_colored_json(&self) -> Result<String, serde_json::Error> {
        let json_value = self.to_json_value()?;
        let json_str = to_colored_json_auto(&json_value)?;

        Ok(json_str)
    }

}

impl CmdArgInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, &'static str) {
        match self {
            CmdArgInputValidationError::MissingProject => (
                "Project not specified.",
                "Use '-p/--project' argument to specify the project.",
            ),
            CmdArgInputValidationError::DuplicateProject => (
                "Project specified multiple times.",
                "Use '-p/--project' argument only once.",
            ),
            CmdArgInputValidationError::MissingEnvironment => (
                "Environment not specified.",
                "Use '-e/--environment' argument to specify the environment.",
            ),
            CmdArgInputValidationError::DuplicateEnvironment => (
                "Environment specified multiple times.",
                "Use '-e/--environment' argument only once.",
            ),
            CmdArgInputValidationError::MissingProjectEnvironment => (
                "Project and environment not specified.",
                "Use '-p/--project' and '-e/--environment' arguments.",
            ),
        }
    }
}

impl ProjectInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            ProjectInputValidationError::NameTooShort { is_root } => {
                if *is_root {
                    ("Argument name is too short.", Some("Minimum is 2 characters."))
                } else {
                    ("Project argument is too short.", Some("Minimum is 2 characters."))
                }
            }
            ProjectInputValidationError::NameTooLong { is_root } => {
                if *is_root {
                    ("Argument name is too long.", Some("Maximum is 40 characters."))
                } else {
                    ("Project argument is too long.", Some("Maximum is 40 characters."))
                }
            }
            ProjectInputValidationError::NameFormat { is_root } => {
                if *is_root {
                    ("Argument name is invalid.", Some("Name can contain only alphanumeric characters, hyphens or underscores (no spaces)."))
                } else {
                    ("Argument project is invalid.", Some("Project name can contain only alphanumeric characters, hyphens or underscores."))
                }
            }
            ProjectInputValidationError::NoUpdateFlags => (
                "No update option specified.",
                Some("Use one of: -n (--name), -d (--description).")
            ),
            ProjectInputValidationError::NewNameFormat => (
                "Name option value is invalid.",
                Some("Name can contain only alphanumeric characters, hyphens or underscores (no spaces).")
            ),
            ProjectInputValidationError::NewNameTooShort => (
                "Name option value is too short.",
                Some("Minimum is 2 characters.")
            ),
            ProjectInputValidationError::NewNameEqualsOriginal => (
                "New name equals to original name.",
                Some("Use different new name.")
            ),
            ProjectInputValidationError::SearchTooShort => (
                "Argument search is too short.",
                Some("Minimum is 2 characters.")
            ),
            ProjectInputValidationError::SearchFormat => (
                "Argument search is invalid.",
                Some("Search can contain only alphanumeric characters, hyphens or underscores.")
            ),
            ProjectInputValidationError::InvalidIdentifierFormat { is_root } => {
                if *is_root {
                    ("Argument name or id is invalid.", Some("The name or id must be alphanumeric, name may include underscores (_) and hyphens(-) and must be between 2 to 40 characters long. Id must start with the prefix 'proj_' followed by 22 alphanumeric characters."))
                } else {
                    ("Argument project is invalid.", Some("The project name or id must be alphanumeric, name may include underscores (_) and hyphens(-) and must be between 2 to 40 characters long. Id must start with the prefix 'proj_' followed by 22 alphanumeric characters."))
                }
            }
            ProjectInputValidationError::NameUsingIdFormat => (
                "Name is using id format.",
                Some("Ensure the name is in a valid format: alphanumeric, may include underscores (_) and hyphens (-), without the prefix 'proj_' followed by 22 alphanumeric characters, min 2 max 40 characters.")
            ),
            ProjectInputValidationError::NewNameTooLong => (
                "Name option value is too long.",
                Some("Maximum is 40 characters.")
            ),
            ProjectInputValidationError::InvalidLimit => (
                "Limit option value is invalid.",
                Some("Limit can range from 2 to 30.")
            ),
            ProjectInputValidationError::InvalidPage => (
                "Page option value is invalid.",
                Some("Page can range from 1 to 1000.")
            ),
        }
    }
}

impl WebhookInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            WebhookInputValidationError::NoUpdateFlags => (
                "No update flag specified.",
                Some("Use one of: -u (--url), -d (--description)."),
            ),
            WebhookInputValidationError::InvalidLimit => (
                "Invalid '--limit' option value.",
                Some("Limit can range from 2 to 30."),
            ),
            WebhookInputValidationError::InvalidId => (
                "Invalid webhook id value.",
                Some(
                    "Id must start with the prefix 'whk_' followed by 22 alphanumeric characters.",
                ),
            ),
            WebhookInputValidationError::InvalidUrl => (
                "Invalid webhook url.",
                Some("Must be valid url using https protocol."),
            ),
            WebhookInputValidationError::DescriptionTooLong => (
                "Description is too long.",
                Some("Maximum is 200 characters."),
            ),
            WebhookInputValidationError::InvalidPage => (
                "Page option value is invalid.",
                Some("Page can range from 1 to 1000."),
            ),
        }
    }
}

impl SecretsInputValidationError {
    pub fn message_and_hint_and_secrets(&self) -> (&'static str, Option<&'static str>, Vec<String>) {
        match self {
            SecretsInputValidationError::NoSecretsToCreate => (
                "No secrets to create provided.",
                Some("Provide at least one secret name."),
                vec![]
            ),
            SecretsInputValidationError::NoSecretsToDelete => (
                "No secrets to delete provided.",
                Some("Provide at least one secret name."),
                vec![]
            ),
            SecretsInputValidationError::NoSecretsToSet => (
                "No secrets to set provided.",
                Some("Provide at least one secret name."),
                vec![]
            ),
            SecretsInputValidationError::NoUpdatesProvided => (
                "No secret updates provided.",
                Some("Provide at least one of the following options: --values, --names, --comments."),
                vec![]
            ),
            SecretsInputValidationError::NamesFormat(secrets) => (
                "Invalid secret names provided.",
                Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed"),
                secrets.clone()
            ),
            SecretsInputValidationError::NamesTooShort(secrets) => (
                "Secret names are too short.",
                Some("Minimum length for secret name is 2 characters."),
                secrets.clone()
            ),
            SecretsInputValidationError::NamesTooLong(secrets) => (
                "Secret names are too long.",
                Some("Maximum length for secret name is 255 characters."),
                secrets.clone()
            ),
            SecretsInputValidationError::CommentsTooLong(secrets) => (
                "Secret comments are too long.",
                Some("Maximum length for comment is 512 characters (after formatting)."),
                secrets.clone()
            ),
            SecretsInputValidationError::CommentTooLong => (
                "Secret comment is too long.",
                Some("Maximum length for comment is 512 characters (after formatting)."),
                vec![]
            ),
            SecretsInputValidationError::SearchFormat => (
                "Argument search is invalid.",
                Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed."),
                vec![]
            ),
            SecretsInputValidationError::SearchTooShort => (
                "Argument search is too short.",
                Some("Minimum is 2 characters."),
                vec![]
            ),
            SecretsInputValidationError::NoNames => (
                "No secrets names specified.",
                Some("Separate names of secrets to return with spaces."),
                vec![]
            ),
            SecretsInputValidationError::DuplicateNames(secrets) => (
                "Found duplicate secret names.",
                Some("Secret names cannot be used more than once."),
                secrets.clone()
            ),
            SecretsInputValidationError::DuplicateNewNames(secrets) => (
                "Found duplicate new names.",
                Some("New names cannot be used more than once."),
                secrets.clone()
            ),
            SecretsInputValidationError::SelfReferences(secrets) => (
                "Found self-referencing secrets.",
                Some("Secrets cannot reference themselves."),
                secrets.clone()
            ),
            SecretsInputValidationError::ReadFile(_) => (
                "Error reading file.",
                None,
                vec![]
            ),
            SecretsInputValidationError::SearchBothNameAndValue => (
                "Cannot provide both 'name' and 'value' options.",
                Some("Provide only one of them."),
                vec![]
            ),
            SecretsInputValidationError::SearchMissingNameOrValue => (
                "No search criteria provided.",
                Some("Provide either 'name' or 'value' option."),
                vec![]
            ),
            SecretsInputValidationError::SearchValueTooLong => (
                "Option 'value' is too long.",
                Some("Maximum length is 1000 characters."),
                vec![]
            ),
            SecretsInputValidationError::SearchValueEmpty => (
                "Option 'value' is empty.",
                Some("Provide non-empty string value."),
                vec![]
            ),
            SecretsInputValidationError::ValuesTooLong(secrets) => (
                "Secret values are too long.",
                Some("Maximum length is 4096 characters."),
                secrets.clone()
            ),
            SecretsInputValidationError::NoData => (
                "No data provided.",
                Some("Provide valid secret data."),
                vec![]
            ),
            SecretsInputValidationError::NewNamesFormat(secrets) => (
                "Invalid new secret names.",
                Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed."),
                secrets.clone()
            ),
            SecretsInputValidationError::MissingPropertiesToUpdate(secrets) => (
                "Missing properties to update.",
                Some("Provide valid secret data."),
                secrets.clone()
            ),
            SecretsInputValidationError::NewNameSameAsName(secrets) => (
                "New name equals to original name.",
                Some("Use different new name."),
                secrets.clone()
            ),
            SecretsInputValidationError::NameValueSeparator => (
                "Invalid name-value pairs.",
                Some("Expected a name-value pairs separated by '='."),
                vec![]
            ),
        }
    }
}

impl EnvironmentsInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            EnvironmentsInputValidationError::NameTooShort { is_root } => {
                if *is_root {
                    ("Argument name is too short.", Some("Minimum is 2 characters."))
                } else {
                    ("Environment argument is too short.", Some("Minimum is 2 characters."))
                }
            }
            EnvironmentsInputValidationError::NameFormat { is_root } => {
                if *is_root {
                    ("Argument name is invalid.", Some("Name can contain only alphanumeric characters, underscores or hyphen separator (no spaces)."))
                } else {
                    ("Argument environment is invalid.", Some("Environment name can contain only alphanumeric characters, underscores or hyphen separator."))
                }
            }
            EnvironmentsInputValidationError::NewNameEqualsOriginal => (
                "Provided new name equals to original name.",
                Some("Use different new name.")
            ),
            EnvironmentsInputValidationError::NoUpdateFlags => (
                "No update flag specified.",
                Some("Use one of: -n (--name), -d (--description), -t (--type).")
            ),
            EnvironmentsInputValidationError::NewNameFormat => (
                "New name option value is invalid.",
                Some("Name can contain only alphanumeric characters, underscores or hyphen separator (no spaces).")
            ),
            EnvironmentsInputValidationError::NewNameTooShort => (
                "New name option value is too short.",
                Some("Minimum is 2 characters.")
            ),
            EnvironmentsInputValidationError::SearchTooShort => (
                "Argument search is too short.",
                Some("Minimum is 2 characters.")
            ),
            EnvironmentsInputValidationError::SearchFormat => (
                "Argument search is invalid.",
                Some("Search can contain only alphanumeric characters, underscores or hyphen separator.")
            ),
            EnvironmentsInputValidationError::InvalidIdentifierFormat { is_root } => {
                if *is_root {
                    ("Argument name or id is invalid.", Some("The name or id must be alphanumeric, name may include underscores (_) and a signle hyphen (-) as as separator and must be between 2 to 40 characters long. Id must start with the prefix 'env_' followed by 22 alphanumeric characters."))
                } else {
                    ("Argument environment is invalid.", Some("The environment name or id must be alphanumeric, name may include underscores (_) and a signle hyphen (-) as as separator and must be between 2 to 40 characters long. Id must start with the prefix 'env_' followed by 22 alphanumeric characters."))
                }
            }
            EnvironmentsInputValidationError::NameUsingIdFormat => (
                "Name is using id format.",
                Some("Ensure the name is in a valid format: alphanumeric, may include underscores (_) and a signle hyphen (-) as as separator, without the prefix 'env_' followed by 22 alphanumeric characters, min 2 max 40 characters.")
            ),
            EnvironmentsInputValidationError::NameTooLong { is_root } => {
                if *is_root {
                    ("Argument name is too long.", Some("Maximum is 40 characters."))
                } else {
                    ("Project argument is too long.", Some("Maximum is 40 characters."))
                }
            }
            EnvironmentsInputValidationError::NewNameTooLong => (
                "New name option value is too long.",
                Some("Maximum is 40 characters.")
            ),
            EnvironmentsInputValidationError::SelfComparison => (
                "Cannot compare an environment with itself.",
                Some("Use different environment for comparison.")
            ),
        }
    }
}

impl YamlEnvConfigError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            YamlEnvConfigError::FileNotFound { custom_path } => match custom_path {
                true => (
                    "No config file found.",
                    Some("Make sure the specified file exists.")
                ),
                false => (
                    "No 'stashbase.yaml' file found.",
                    Some("Create file or use '-p' and '-e' flags.")
                ),
            },
            YamlEnvConfigError::NoEntries => (
                "No entries found in 'stashbase.yaml'.",
                Some("Add entries to the file or use '-p' and '-e' flags.")
            ),
            YamlEnvConfigError::FailedToRead {
                custom_path,
                message,
            } => match custom_path {
                true => (
                    "Failed to read the specified config file.",
                    Some(Box::leak(message.clone().into_boxed_str()))
                ),
                false => (
                    "Failed to read 'stashbase.yaml' file.",
                    Some(Box::leak(message.clone().into_boxed_str()))
                ),
            },
        }
    }
}

impl LoadEnvironmentInputValidationError {
    pub fn message_and_hint_and_secrets(&self) -> (&'static str, Option<&'static str>, Vec<String>) {
        match self {
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly => (
                "Use of both --exclude and --only flag.",
                Some("Use only one of them."),
                vec![]
            ),
            LoadEnvironmentInputValidationError::OnlySecretNamesFormat(secrets) => (
                "Invalid only secret names.",
                Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::OnlySecretNamesTooShort(secrets) => (
                "Only argument secret names are too short.",
                Some("Minimum length for secret name is 2 characters."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::OnlySecretNamesTooLong(secrets) => (
                "Only argument secret names are too long.",
                Some("Maximum length for secret name is 255 characters."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::ExcludeSecretNamesFormat(secrets) => (
                "Invalid exclude secret names.",
                Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::ExcludeSecretNamesTooShort(secrets) => (
                "Exclude secret names are too short.",
                Some("Minimum length for secret name is 2 characters."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::ExcludeSecretNamesTooLong(secrets) => (
                "Exclude secret names are too long.",
                Some("Maximum length for secret name is 255 characters."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::MissingProjectArg => (
                "Missing project argument.",
                Some("Use '-p' flag to specify the project."),
                vec![]
            ),
            LoadEnvironmentInputValidationError::MissingEnvArg => (
                "Missing environment argument.",
                Some("Use '-e' flag to specify the environment."),
                vec![]
            ),
            LoadEnvironmentInputValidationError::FileArgWithInline => (
                "Cannot use '--file' flag and '-p' or '-e' flag at the same time.",
                None,
                vec![]
            ),
            LoadEnvironmentInputValidationError::SetSecretNameValueSeparator => (
                "Invalid set argument.",
                Some("Expected a name-value pair (separated by '=')."),
                vec![]  
            ),
            LoadEnvironmentInputValidationError::SetSecretNamesFormat(secrets) => (
                "Invalid set secret names.",
                Some("Cannot start with a digit, only uppercase alphanumeric characters and underscores allowed."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::SetSecretNamesTooShort(secrets) => (
                "Set secret names are too short.",
                Some("Minimum length for secret name is 2 characters."),
                secrets.clone()
            ),
            LoadEnvironmentInputValidationError::SetSecretNamesTooLong(secrets) => (
                "Set secret names are too long.",
                Some("Maximum length for secret name is 255 characters."),
                secrets.clone()
            ),
        }
    }
}

impl PushPullInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            PushPullInputValidationError::NoFileSpecified { is_push } => match is_push {
                true => (
                    "No file specified.",
                    Some("Add root property 'file' or push property 'file' to the config or use '--file' flag.")
                ),
                false => (
                    "No file specified.",
                    Some("Add root property 'file' or pull property 'file' to the config or use '--file' flag.")
                ),
            },
        }
    }
}

impl RunInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            RunInputValidationError::NoCmdProvided => (
                "No command provided.",
                Some("Provide command you want to run."),
            ),
            RunInputValidationError::NoSecretsToFetch => (
                "No secrets to fetch.",
                Some("'set' secrets option overrides all secrets from option 'only'."),
            ),
        }
    }
}

impl ScanInputValidationError {
    pub fn message_and_hint(&self) -> (&'static str, Option<&'static str>) {
        match self {
            ScanInputValidationError::BaselineFileNotFound { path: _ } => (
                "Baseline file not found.",
                Some("Check that the baseline file path is correct and the file exists."),
            ),
            ScanInputValidationError::BaselineFileRead { path: _, message: _ } => (
                "Failed to read baseline file.",
                Some("Check file permissions and ensure the file is accessible."),
            ),
            ScanInputValidationError::BaselineFileParse { path: _, message: _ } => (
                "Failed to parse baseline file.",
                Some("Ensure the baseline file contains valid JSON scan results."),
            ),
            ScanInputValidationError::GitRepositoryNotFound => (
                "Git repository not found.",
                Some("Make sure you are in a Git repository directory."),
            ),
            ScanInputValidationError::GitRepositoryAccess { message: _ } => (
                "Failed to access Git repository.",
                Some("Check repository permissions and try again."),
            ),
            ScanInputValidationError::GitIndexAccess { message: _ } => (
                "Failed to access Git index.",
                Some("Check if the repository is in a valid state."),
            ),
            ScanInputValidationError::GitHeadAccess { message: _ } => (
                "Failed to access Git HEAD.",
                Some("Check if the repository has commits or is in a valid state."),
            ),
            ScanInputValidationError::GitBranchAccess { message: _ } => (
                "Failed to access Git branch.",
                Some("Check if the branch exists and repository is in a valid state."),
            ),
            ScanInputValidationError::GitCommitAccess { message: _ } => (
                "Failed to access Git commit.",
                Some("Check if the commit exists and repository is in a valid state."),
            ),
            ScanInputValidationError::GitTreeAccess { message: _ } => (
                "Failed to access Git tree.",
                Some("Check if the repository structure is valid."),
            ),
            ScanInputValidationError::GitDiffGeneration { message: _ } => (
                "Failed to generate Git diff.",
                Some("Check if there are valid changes to process."),
            ),
            ScanInputValidationError::GitDiffProcessing { message: _ } => (
                "Failed to process Git diff.",
                Some("Check if the diff format is valid."),
            ),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MessageHint {
    message: &'static str,
    hint: Option<&'static str>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    secrets: Vec<String>,
}

impl InputValidationError {
    pub fn to_struct(&self) -> MessageHint {
        return match self {
            InputValidationError::CmdArgs(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: Some(h), secrets: vec![] }
            }
            InputValidationError::Projects(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
            InputValidationError::Secrets(inner) => {
                let (m, h, s) = inner.message_and_hint_and_secrets();
                MessageHint { message: m, hint: h, secrets: s }
            }
            InputValidationError::Environments(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
            InputValidationError::YamlConfigFile(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
            InputValidationError::Run(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
            InputValidationError::LoadEnvironment(inner) => {
                let (m, h, s) = inner.message_and_hint_and_secrets();
                MessageHint { message: m, hint: h, secrets: s }
            }
            InputValidationError::PushPullEnvironment(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
            InputValidationError::Webhook(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
            InputValidationError::Scan(inner) => {
                let (m, h) = inner.message_and_hint();
                MessageHint { message: m, hint: h, secrets: vec![] }
            }
        };

    }

}
