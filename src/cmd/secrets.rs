use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use super::shared::{try_get_project_environment, SharedProjectEnvArgs};

#[derive(Debug, Args)]
#[command(override_usage = "secrets <COMMAND> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct SecretArgs {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,

    #[clap(subcommand)]
    pub subcommand: SecretSubcommand,
}

impl SecretArgs {
    pub fn try_get_project_environment(&self) -> Result<(String, String)> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

        try_get_project_environment(root_project, root_environment, project, environment)
    }
}

impl SecretSubcommand {
    pub fn get_project_environment(&self) -> (Option<&str>, Option<&str>) {
        match &self {
            SecretSubcommand::List(l) => (
                l.shared_args.project.as_deref(),
                l.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Get(g) => (
                g.shared_args.project.as_deref(),
                g.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Set(s) => (
                s.shared_args.project.as_deref(),
                s.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Upload(u) => (
                u.shared_args.project.as_deref(),
                u.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Rename(r) => (
                r.shared_args.project.as_deref(),
                r.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Description(d) => (
                d.shared_args.project.as_deref(),
                d.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Delete(d) => (
                d.shared_args.project.as_deref(),
                d.shared_args.environment.as_deref(),
            ),
        }
    }
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
#[command(override_usage = "secrets list -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct ListSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Project description
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,

    /// Print only keys
    #[arg(value_enum, long = "only-keys")]
    pub only_keys: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets get [KEYS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct GetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // #[clap(short='v', long="k", value_parser, num_args = 1.., value_delimiter = ' ')]
    pub keys: Vec<String>,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFromat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets DELETE [KEYS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct DeleteSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets (keys) to delete
    #[clap(value_parser, num_args = 1.., value_delimiter = ' ')]
    pub keys: Vec<String>,

    /// Delete all secrets
    #[arg(name = "all", value_enum, long = "all")]
    pub delete_all: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets SET [SECRETS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct SetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets to set: KEY_1=VAL_1 KEY_2=VAL_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,

    /// Descriptions to set: KEY_1=NOTE_1 KEY_2=NOTE_2
    // #[clap(value_parser, long="description", short='d', num_args = 1.., value_delimiter = ' ')]
    #[clap(value_parser, long="description", short='d', num_args = 1..)]
    pub descriptions: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets upload <FILE_PATH> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct UploadSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // NOTE: for now only accepts .env
    /// Path to file (dotenv format)
    pub file_path: String,
}

#[derive(Debug, Args)]
#[command(
    override_usage = "secrets description <KEY> <DESCRIPTION> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]"
)]
pub struct SetDescription {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secret key
    pub key: String,

    /// Description
    pub description: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets rename [SECETS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct RenameSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets to rename: KEY_1=NEW_KEY_1 KEY_2=NEW_KEY_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum SecretsFromat {
    List,
    Dotenv,
    Table,
    Json,
}
