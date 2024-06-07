use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, Args)]
#[command(override_usage = "config <COMMAND> [OPTIONS]")]
pub struct ConfigCommands {
    #[clap(subcommand)]
    pub subcommand: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Set values
    #[clap(alias = "s")]
    Set(SetConfig),
}

#[derive(Debug, Args)]
#[command(override_usage = "config set <COMMAND> [OPTIONS]")]
pub struct SetConfig {
    #[clap(subcommand)]
    pub subcommand: SetConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SetConfigSubcommand {
    #[clap(alias = "t")]
    /// Set api_key
    ApiKey(SetApiKey),
    /// Set default output format
    Output(SetOutput),
    OutputSecrets(SetOutputSecrets),
}

#[derive(Debug, Args)]
#[command(override_usage = "config set api-key <VALUE> [OPTIONS]")]
pub struct SetApiKey {
    pub value: String,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    List,
    Table,
    Json,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SecretsOutputFormat {
    #[default]
    List,
    Dotenv,
    Table,
    Json,
}

#[derive(Debug, Args)]
#[command(override_usage = "config set output <FORMAT> [OPTIONS]")]
pub struct SetOutput {
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
#[command(override_usage = "config set output-secrets <FORMAT> [OPTIONS]")]
pub struct SetOutputSecrets {
    pub format: SecretsOutputFormat,
}
