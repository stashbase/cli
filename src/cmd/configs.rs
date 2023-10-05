use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ConfigCommands {
    #[clap(subcommand)]
    pub subcommand: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Set values
    Set(SetConfig),
}

#[derive(Debug, Args)]
pub struct SetConfig {
    #[clap(subcommand)]
    pub subcommand: SetConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SetConfigSubcommand {
    /// Set token
    Token(SetToken),
}

#[derive(Debug, Args)]
pub struct SetToken {
    pub value: String,
}
