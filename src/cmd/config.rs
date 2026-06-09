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
    // add alias for dotenv
    #[clap(alias = ".env")]
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

    /// Expand secrets references to their values
    ExpandRefs(ExpandRefsCommand),

    /// Print current config
    Print(PrintConfig),
    /// Reset config file
    Reset(ResetConfig),
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
#[command(override_usage = "config api-key set [OPTIONS]")]
pub struct SetApiKey {
    /// Read API key from stdin instead of prompting interactively
    #[arg(long = "stdin")]
    pub stdin: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "config api-key print [OPTIONS]")]
pub struct PrintApiKey {}

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
#[command(override_usage = "config output set <FORMAT> [OPTIONS]")]
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
#[command(override_usage = "config output-secrets set <FORMAT> [OPTIONS]")]
pub struct SetSecretsOutput {
    pub format: SecretsOutputFormat,
}

#[derive(Debug, Args)]
pub struct ExpandRefsCommand {
    #[clap(subcommand)]
    pub subcommand: ExpandRefsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ExpandRefsSubcommand {
    /// Set default expand-refs (secrets)
    Set(SetExpandRefs),

    /// Print default expand-refs (secrets)
    Print,
}

#[derive(Debug, Args)]
#[command(override_usage = "config expand-refs set <ENABLED> [OPTIONS]")]
pub struct SetExpandRefs {
    pub enabled: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "config reset [OPTIONS]")]
pub struct ResetConfig {
    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "config print [OPTIONS]")]
pub struct PrintConfig {
    #[arg(long = "show-sensitive")]
    pub show_sensitive: bool,
}
