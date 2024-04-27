use clap::{Args, Subcommand};

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
}

#[derive(Debug, Args)]
#[command(override_usage = "config set api-key <VALUE> [OPTIONS]")]
pub struct SetApiKey {
    pub value: String,
}
