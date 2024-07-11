use std::fmt::Display;

use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    List,
    Table,
    Json,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::List => write!(f, "list"),
            Self::Table => write!(f, "table"),
            Self::Json => write!(f, "json"),
        }
    }
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretsOutputFormat {
    #[default]
    List,
    Table,
    Dotenv,
    Yaml,
    Json,
}

impl Display for SecretsOutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::List => write!(f, "list"),
            Self::Dotenv => write!(f, "dotenv"),
            Self::Table => write!(f, "table"),
            Self::Json => write!(f, "json"),
            Self::Yaml => write!(f, "yaml"),
        }
    }
}

#[derive(Debug, Args)]
#[command(override_usage = "config <COMMAND> [OPTIONS]")]
pub struct ConfigCommand {
    #[clap(subcommand)]
    pub subcommand: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Api key config
    ApiKey(ApiKeyCommand),
    /// Default output (general)
    Output(OutputCommand),
    /// Default output for secrets
    OutputSecrets(SecretsOutputCommand),
    /// Print current config
    Print,
    /// Reset config file
    Reset,
}

#[derive(Debug, Args)]
pub struct ApiKeyCommand {
    #[clap(subcommand)]
    pub subcommand: ApiKeySubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ApiKeySubcommand {
    /// Set api key
    Set(SetApiKey),

    /// Print api key
    Print(PrintApiKey),
}

#[derive(Debug, Args)]
#[command(override_usage = "config set api-key <VALUE> [OPTIONS]")]
pub struct SetApiKey {
    pub value: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "config api-key print [OPTIONS]")]
pub struct PrintApiKey {
    /// Print full api key token
    #[arg(value_enum, long = "full")]
    pub full: bool,
}

//

#[derive(Debug, Args)]
pub struct OutputCommand {
    #[clap(subcommand)]
    pub subcommand: OutputSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum OutputSubcommand {
    /// Set api_key
    Set(SetOutput),

    /// Print current default output format
    Print,
}

#[derive(Debug, Args)]
#[command(override_usage = "config set output <FORMAT> [OPTIONS]")]
pub struct SetOutput {
    pub format: OutputFormat,
}

//

#[derive(Debug, Args)]
pub struct SecretsOutputCommand {
    #[clap(subcommand)]
    pub subcommand: SecretsOutputSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SecretsOutputSubcommand {
    /// Set api key
    Set(SetSecretsOutput),

    /// Print current default output format
    Print,
}

#[derive(Debug, Args)]
#[command(override_usage = "config set output-secrets <FORMAT> [OPTIONS]")]
pub struct SetSecretsOutput {
    pub format: SecretsOutputFormat,
}
