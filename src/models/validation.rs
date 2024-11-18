use core::fmt;
use owo_colors::OwoColorize;

use crate::utils::validation::{SECRET_DESCRIPTION_MAX_LENGTH, SECRET_VALUE_MAX_LENGTH};

#[derive(Debug)]
pub enum InputValidationError {
    CmdArgs(CmdArgInputValidationError),
    Projects(ProjectInputValidationError),
    Secrets(SecretsInputValidationError),
    Environments(EnvironmentsInputValidationError),
    EnvChangelog(EnvChangelogInputValidationError),
    YamlConfigFile(YamlEnvConfigError),
    Run(RunInputValidationError),
    LoadEnvironment(LoadEnvironmentInputValidationError),
    PushPullEnvironment(PushPullInputValidationError),
    Webhook(WebhookInputValidationError),
}

#[derive(Debug)]
pub enum CmdArgInputValidationError {
    MissingProject,
    DuplicateProject,
    MissingEnvironment,
    DuplicateEnvironment,
    MissingProjectEnvironment,
}

#[derive(Debug)]
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

#[derive(Debug)]
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
#[derive(Debug)]
pub enum SecretsInputValidationError {
    NoNames,
    NameFormat { multiple: bool },
    NameTooShort { multiple: bool },
    NameTooLong { multiple: bool },
    DuplicateNames(Vec<String>),
    DuplicateNewNames(Vec<String>),
    SelfReferences(Vec<String>),
    ReadFile(anyhow::Error),
    DescriptionTooLong,
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
}

// TODO: check if is used as value (env cmd) or as arg (secrets cmd)
#[derive(Debug)]
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

#[derive(Debug)]
pub enum EnvChangelogInputValidationError {
    // InvalidIdFormat,
    // InvalidIdLength,
    InvalidId,
    InvalidLimit,
    InvalidPage,
}

// geenral for stashbase.yaml file, shared between load, push and pull commands
#[derive(Debug)]
pub enum YamlEnvConfigError {
    FileNotFound { custom_path: bool },
    FailedToRead { custom_path: bool, message: String },
    NoEntries,
}

#[derive(Debug)]
pub enum LoadEnvironmentInputValidationError {
    FileArgWithInline,
    MissingProjectArg,
    MissingEnvArg,
    UseOfBothExcludeAndOnly,
    OnlyNameFormat,
    ExcludeNameFormat,
    SetNameValueSeparator,
    SetNameValueFormat,
}

#[derive(Debug)]
pub enum PushPullInputValidationError {
    NoFileSpecified { is_push: bool },
    // other errors same as from LoadEnvironment
}

#[derive(Debug)]
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
                msg = "project not specified";
                hint = "use '-p/--project' argument to specify the project";
            }
            CmdArgInputValidationError::DuplicateProject => {
                msg = "project specified multiple times";
                hint = "use '-p/--project' argument only once";
            }
            CmdArgInputValidationError::MissingEnvironment => {
                msg = "environment not specified";
                hint = "use '-e/--environment' argument to specify the environment";
            }
            CmdArgInputValidationError::DuplicateEnvironment => {
                msg = "environment specified multiple times";
                hint = "use '-e/--environment' argument only once";
            }
            CmdArgInputValidationError::MissingProjectEnvironment => {
                msg = "project and environment not specified";
                hint = "use '-p/--project' and '-e/--environment' arguments";
            }
        }

        writeln!(f, "{}", format!("- message: {}", msg))?;
        write!(f, "{}", format!("- hint: {}", hint))?;

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
                    msg = "argument name is too short";
                    hint = Some("minimum is 2 characters");
                } else {
                    msg = "project argument is too short";
                    hint = Some("minimum is 2 characters");
                }
            }

            ProjectInputValidationError::NameTooLong { is_root } => {
                if *is_root {
                    msg = "argument name is too long";
                    hint = Some("maximum is 40 characters");
                } else {
                    msg = "project argument is too long";
                    hint = Some("maximum is 40 characters");
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
                msg = "no update option specified";
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
            ProjectInputValidationError::NewNameEqualsOriginal => {
                msg = "new name equals to original name";
                hint = Some("use different new name");
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
            ProjectInputValidationError::InvalidIdentifierFormat { is_root } => {
                if *is_root {
                    let  hint_str = "The name or id must be alphanumeric, name may include underscores (_) and hyphens(-) and must be between 2 to 40 characters long. Id must start with the prefix 'proj_' followed by 22 alphanumeric characters.";

                    msg = "argument name or id is invalid";
                    hint = Some(&hint_str);
                } else {
                    let  hint_str = "The project name or id must be alphanumeric, name may include underscores (_) and hyphens(-) and must be between 2 to 40 characters long. Id must start with the prefix 'proj_' followed by 22 alphanumeric characters.";

                    msg = "argument project is invalid";
                    hint = Some(&hint_str);
                }
            }
            ProjectInputValidationError::NameUsingIdFormat => {
                let hint_str = "Ensure the name is in a valid format: alphanumeric, may include underscores (_) and hyphens (-), without the prefix 'proj_' followed by 22 alphanumeric characters, min 2 max 40 characters.";

                msg = "name is using id format";
                hint = Some(&hint_str);
            }
            ProjectInputValidationError::NewNameTooLong => {
                msg = "name option value is too long";
                hint = Some("maximum is 40 characters");
            }
            ProjectInputValidationError::InvalidLimit => {
                msg = "limit option value is invalid";
                hint = Some("limit can range from 2 to 30");
            }
            ProjectInputValidationError::InvalidPage => {
                msg = "page option value is invalid";
                hint = Some("page can range from 1 to 1000");
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
            SecretsInputValidationError::NameFormat { multiple } => {
                let message = match multiple {
                    true => "invalid secret names",
                    false => "invalid secret name",
                };
                msg = message;
                hint = Some(
                    "cannot start with a digit, only uppercase alphanumeric characters and underscores allowed",
                );
            }
            SecretsInputValidationError::NameTooShort { multiple } => {
                let message = match multiple {
                    true => "secret names are too short",
                    false => "secret name is too short",
                };
                msg = message;
                hint = Some("mimimal length for secret name is 2 characters");
            }

            SecretsInputValidationError::NameTooLong { multiple } => {
                let message = match multiple {
                    true => "secret names are too long",
                    false => "secret name is too long",
                };
                msg = message;
                hint = Some("maximum length for secret name is 255 characters");
            }
            SecretsInputValidationError::DescriptionTooLong => {
                let msg = "secret description is too long";

                let hint_str = format!(
                    "maximum length for description is {} characters (after formatting)",
                    SECRET_DESCRIPTION_MAX_LENGTH
                );

                writeln!(f, "{}", format!("- message: {}", msg))?;
                write!(f, "{}", format!("- hint: {}", hint_str))?;

                return Ok(());
            }
            SecretsInputValidationError::SearchFormat => {
                msg = "argument search is invalid";
                hint = Some(
                    "cannot start with a digit, only uppercase alphanumeric characters and underscores allowed",
                );
            }
            SecretsInputValidationError::SearchTooShort => {
                msg = "argument search is too short";
                hint = Some("minimum is 2 characters");
            }
            SecretsInputValidationError::NoNames => {
                msg = "no secrets names specified";
                hint = Some("separate names of secrets to return with spaces");
            }
            SecretsInputValidationError::DuplicateNames(names) => {
                let names_str = names.join(", ");

                writeln!(f, "{}", format!("- message: {}", "found duplicate names"))?;
                write!(f, "{}", format!("- duplicates: {}", names_str))?;

                return Ok(());
            }
            SecretsInputValidationError::DuplicateNewNames(names) => {
                let names_str = names.join(", ");

                let msg = "found duplicate new names";

                writeln!(f, "- message: {}", msg)?;
                write!(f, "{}", format!("- duplicates: {}", names_str))?;

                return Ok(());
            }
            SecretsInputValidationError::SelfReferences(names) => {
                let names_str = names.join(", ");
                let msg = "found self-referencing secrets";

                writeln!(f, "{}", format!("- message: {}", msg))?;
                write!(f, "{}", format!("- secrets: {}", names_str))?;

                return Ok(());
            }
            SecretsInputValidationError::ReadFile(error) => {
                let msg = "error reading file";

                writeln!(f, "- message: {}", msg)?;
                write!(f, "{}", format!("- details: {}", error.to_string()))?;

                return Ok(());
            }
            SecretsInputValidationError::SearchBothNameAndValue => {
                msg = "cannot provide both 'name' and 'value' options";
                hint = Some("provide only one of them");
            }
            SecretsInputValidationError::SearchMissingNameOrValue => {
                msg = "no search criteria provided";
                hint = Some("provide either 'name' or 'value' option");
            }
            SecretsInputValidationError::SearchValueTooLong => {
                msg = "option 'value' is too long";
                hint = Some("maximum length is 1000 characters");
            }
            SecretsInputValidationError::SearchValueEmpty => {
                msg = "option 'value' is empty";
                hint = Some("provide non-empty string value");
            }
            SecretsInputValidationError::ValuesTooLong(secret_names) => {
                let names_str = secret_names.join(", ");
                let msg = format!("secret values are too long",);

                let hint_str = format!(
                    "maximum length for secret value is {} characters",
                    SECRET_VALUE_MAX_LENGTH
                );

                writeln!(f, "{}", format!("- message: {}", msg))?;
                writeln!(f, "{}", format!("- hint: {}", hint_str))?;
                write!(f, "{}", format!("- secrets: {}", names_str))?;

                return Ok(());
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
                        "name can contain only alphanumeric characters, underscores or hyphen separator (no spaces)",
                    );
                } else {
                    msg = "argument environment is invalid";
                    hint = Some("environment name can contain only alphanumeric characters, underscores or hyphen separator");
                }
            }
            EnvironmentsInputValidationError::NewNameEqualsOriginal => {
                msg = "provided new name equals to original name";
                hint = Some("use different new name");
            }
            EnvironmentsInputValidationError::NoUpdateFlags => {
                msg = "no update flag specified";
                hint = Some("use one of: -n (--name), -d (--description), -t (--type)");
            }
            EnvironmentsInputValidationError::NewNameFormat => {
                msg = "new name option value is invalid";
                hint = Some("name can contain only alphanumeric characters, underscores or hyphen separator (no spaces)");
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
                    Some("search can contain only alphanumeric characters, underscores or hyphen separator");
            }
            EnvironmentsInputValidationError::InvalidIdentifierFormat { is_root } => {
                if *is_root {
                    let  hint_str = "The name or id must be alphanumeric, name may include underscores (_) and a signle hyphen (-) as as separator and must be between 2 to 40 characters long. Id must start with the prefix 'env_' followed by 22 alphanumeric characters.";

                    msg = "argument name or id is invalid";
                    hint = Some(&hint_str);
                } else {
                    let  hint_str = "The environment name or id must be alphanumeric, name may include underscores (_) and a signle hyphen (-) as as separator and must be between 2 to 40 characters long. Id must start with the prefix 'env_' followed by 22 alphanumeric characters.";

                    msg = "argument environment is invalid";
                    hint = Some(&hint_str);
                }
            }
            EnvironmentsInputValidationError::NameUsingIdFormat => {
                let hint_str = "Ensure the name is in a valid format: alphanumeric, may include underscores (_) and a signle hyphen (-) as as separator, without the prefix 'env_' followed by 22 alphanumeric characters, min 2 max 40 characters.";

                msg = "name is using id format";
                hint = Some(&hint_str);
            }
            EnvironmentsInputValidationError::NameTooLong { is_root } => {
                if *is_root {
                    msg = "argument name is too long";
                    hint = Some("maximum is 40 characters");
                } else {
                    msg = "project argument is too long";
                    hint = Some("maximum is 40 characters");
                }
            }
            EnvironmentsInputValidationError::NewNameTooLong => {
                msg = "name option value is too long";
                hint = Some("maximum is 40 characters");
            }
            EnvironmentsInputValidationError::SelfComparison => {
                msg = "cannot compare an environment with itself";
                hint = Some("use different environment for comparison");
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
            //     EnvChangelogInputValidationError::InvalidIdFormat => {
            //         msg = "invalid id";
            //         hint = Some("is must be alphanumeric");
            //     }
            //     EnvChangelogInputValidationError::InvalidIdLength => {
            //         msg = "invalid id";
            //         hint = Some("id must be 22 characters long");
            //     }
            EnvChangelogInputValidationError::InvalidId => {
                let hint_str =
                    "Id must start with the prefix 'chng_' followed by 22 alphanumeric characters.";

                msg = "invalid changelog id";
                hint = Some(&hint_str);
            }
            EnvChangelogInputValidationError::InvalidLimit => {
                msg = "limit option value is invalid";
                hint = Some("limit can range from 2 to 30");
            }
            EnvChangelogInputValidationError::InvalidPage => {
                msg = "page option value is invalid";
                hint = Some("page can range from 1 to 1000");
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
            LoadEnvironmentInputValidationError::OnlyNameFormat => {
                msg = "invalid only argument";
                hint = Some("accepts only uppercase alphanumeric characters and underscores");
            }
            LoadEnvironmentInputValidationError::ExcludeNameFormat => {
                msg = "invalid exclude argument";
                hint = Some("accepts only uppercase alphanumeric characters and underscores");
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
            LoadEnvironmentInputValidationError::SetNameValueSeparator => {
                msg = "invalid set argument";
                hint = Some("expected a name-value pair (separated by '=')");
            }
            LoadEnvironmentInputValidationError::SetNameValueFormat => {
                msg = "invalid set argument";
                hint = Some(
                    "secret name can contain only uppercase alphanumeric characters and underscores",
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

impl fmt::Display for PushPullInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            PushPullInputValidationError::NoFileSpecified { is_push } => match is_push {
                true => {
                    msg = "no file specified";
                    hint =
                    Some("add root property 'file' or push property 'file' to the config or use '--file' flag");
                }
                false => {
                    msg = "no file specified";
                    hint =
                    Some("add root property 'file' or push property 'file' to the config or use '--file' flag");
                }
            },
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
            WebhookInputValidationError::InvalidLimit => {
                msg = "invalid '--limit' option value";
                hint = Some("limit can range from 2 to 30");
            }
            WebhookInputValidationError::InvalidId => {
                let hint_str =
                    "Id must start with the prefix 'whk_' followed by 22 alphanumeric characters.";

                msg = "invalid webhook id value";
                hint = Some(&hint_str);
            }
            WebhookInputValidationError::InvalidUrl => {
                msg = "invalid webhook url";
                hint = Some("must be valid url using https protocol");
            }
            WebhookInputValidationError::DescriptionTooLong => {
                msg = "description is too long";
                hint = Some("maximum is 200 characters");
            }
            WebhookInputValidationError::InvalidPage => {
                msg = "page option value is invalid";
                hint = Some("page can range from 1 to 1000");
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

impl fmt::Display for RunInputValidationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            RunInputValidationError::NoCmdProvided => {
                msg = "no command provided";
                hint = Some("provide command you want to run");
            }
            RunInputValidationError::NoSecretsToFetch => {
                msg = "no secrets to fetch";
                hint = Some("'set' secrets option overrides all secrets from option 'only'");
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

impl fmt::Display for YamlEnvConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg: &str;
        let hint: Option<&str>;

        match self {
            YamlEnvConfigError::FileNotFound { custom_path } => match custom_path {
                true => {
                    msg = "no config file found";
                    hint = Some("make sure the specified file exists");
                }
                false => {
                    msg = "no 'stashbase.yaml' file found";
                    hint = Some("create file or use '-p' and '-e' flags");
                }
            },
            YamlEnvConfigError::NoEntries => {
                msg = "no entries found in 'stashbase.yaml'";
                hint = Some("add entries to the file or use '-p' and '-e' flags");
            }
            YamlEnvConfigError::FailedToRead {
                custom_path,
                message,
            } => match custom_path {
                true => {
                    msg = "failed to read the specified config file";
                    hint = Some(message);
                }
                false => {
                    msg = "failed to read 'stashbase.yaml' file";
                    hint = Some(message);
                }
            },
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
            InputValidationError::PushPullEnvironment(inner) => write!(f, "{}", inner),
            InputValidationError::Webhook(inner) => write!(f, "{}", inner),
            InputValidationError::CmdArgs(inner) => write!(f, "{}", inner),
            InputValidationError::Run(inner) => write!(f, "{}", inner),
            InputValidationError::YamlConfigFile(inner) => write!(f, "{}", inner),
        }
    }
}
