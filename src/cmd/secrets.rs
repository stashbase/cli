use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
pub struct SecretArgs {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = true)]
    pub project: String,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = true)]
    pub environment: String,

    #[clap(subcommand)]
    pub subcommand: SecretSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SecretSubcommand {
    /// List secrets
    #[clap(alias = "l")]
    List(ListSecrets),
}

#[derive(Debug, Args)]
pub struct ListSecrets {
    /// Project description
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum SecretsFromat {
    Dotenv,
    Json,
}
