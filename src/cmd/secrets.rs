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

    /// Get secrets
    #[clap(alias = "g")]
    Get(GetSecrets),

    /// Set secrets
    #[clap(alias = "s")]
    Set(SetSecrets),

    /// Upload secrets
    #[clap(alias = "upl")]
    Upload(UploadSecrets),

    /// Rename secrets
    #[clap(alias = "r")]
    Rename(RenameSecrets),

    /// Set description of existing secret
    #[clap(alias = "des")]
    Description(SetDescription),

    /// Delete one or multiple secrets
    #[clap(aliases = &[ "del"])]
    Delete(DeleteSecrets),
}

#[derive(Debug, Args)]
pub struct ListSecrets {
    /// Search key
    #[arg(value_enum, long = "search")]
    pub search: Option<String>,

    /// Project description
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,

    /// Print only keys
    #[arg(value_enum, long = "only-keys")]
    pub only_keys: bool,
}

#[derive(Debug, Args)]
pub struct GetSecrets {
    // #[clap(short='v', long="k", value_parser, num_args = 1.., value_delimiter = ' ')]
    pub keys: Vec<String>,

    /// Format secrets (default list)
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,
}

#[derive(Debug, Args)]
pub struct DeleteSecrets {
    /// Secrets (keys) to delete
    #[clap(value_parser, num_args = 1.., value_delimiter = ' ')]
    pub keys: Vec<String>,

    /// Delete all secrets
    #[arg(name = "all", value_enum, long = "all")]
    pub delete_all: bool,
}

#[derive(Debug, Args)]
pub struct SetSecrets {
    /// Secrets to set: KEY_1=VAL_1 KEY_2=VAL_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,

    /// Descriptions to set: KEY_1=NOTE_1 KEY_2=NOTE_2
    // #[clap(value_parser, long="description", short='d', num_args = 1.., value_delimiter = ' ')]
    #[clap(value_parser, long="description", short='d', num_args = 1..)]
    pub descriptions: Vec<String>,
}

#[derive(Debug, Args)]
pub struct UploadSecrets {
    // NOTE: for now only accepts .env
    /// Path to file (dotenv format)
    pub file_path: String,
}

#[derive(Debug, Args)]
pub struct SetDescription {
    /// Secret key
    pub key: String,

    /// Description
    pub description: String,
}

#[derive(Debug, Args)]
pub struct RenameSecrets {
    /// Secrets to rename: KEY_1=NEW_KEY_1 KEY_2=NEW_KEY_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum SecretsFromat {
    List,
    Dotenv,
    Json,
}
