use std::fmt::Display;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::models::secrets::SecretsSearchOutputFormat;

use super::{
    config::SecretsOutputFormat,
    shared::{try_get_project_environment, SharedProjectEnvArgs},
};

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
            SecretSubcommand::Create(c) => (
                c.shared_args.project.as_deref(),
                c.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Update(u) => (
                u.shared_args.project.as_deref(),
                u.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Upload(u) => (
                u.shared_args.project.as_deref(),
                u.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Rename(r) => (
                r.shared_args.project.as_deref(),
                r.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Comment(c) => (
                c.shared_args.project.as_deref(),
                c.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Delete(d) => (
                d.shared_args.project.as_deref(),
                d.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Search(search_secrets) => (search_secrets.project.as_deref(), None),
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
    #[clap(alias = "upsert")]
    Set(SetSecrets),

    /// Create secrets
    #[clap(alias = "c")]
    Create(CreateSecrets),

    /// Update secrets
    Update(UpdateSecrets),

    /// Upload secrets
    #[clap(alias = "upl")]
    Upload(UploadSecrets),

    /// Rename secrets
    #[clap(alias = "r")]
    Rename(RenameSecrets),

    /// Set comment of existing secret
    #[clap(alias = "com")]
    Comment(SetComment),

    /// Delete one or multiple secrets
    #[clap(aliases = &[ "del"])]
    Delete(DeleteSecrets),

    /// Search secrets by exact name or value
    Search(SearchSecrets),
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets list -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct ListSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Output format
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsOutputFormat>,

    /// Print only names
    #[arg(value_enum, long = "only-names")]
    pub only_names: bool,

    /// Expand references to their values
    #[arg(value_enum, long = "expand-refs")]
    pub expand_refs: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets get [NAMES] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct GetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // #[clap(short='v', long="k", value_parser, num_args = 1.., value_delimiter = ' ')]
    pub names: Vec<String>,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsOutputFormat>,

    /// Expand references to their values
    #[arg(value_enum, long = "expand-refs")]
    pub expand_refs: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets DELETE [NAMES] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct DeleteSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets (names) to delete
    #[clap(value_parser, num_args = 1.., value_delimiter = ' ')]
    pub names: Vec<String>,

    /// Delete all secrets
    #[arg(name = "all", value_enum, long = "all")]
    pub delete_all: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets SET [SECRETS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct SetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets to set: NAME_1=VAL_1 NAME_2=VAL_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,

    /// Comments to set: NAME_1=NOTE_1 NAME_2=NOTE_2
    #[clap(value_parser, long="comment", short='c', num_args = 1..)]
    pub comments: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets CREATE [SECRETS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct CreateSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets to create: NAME_1=VAL_1 NAME_2=VAL_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,

    /// Comments to set: NAME_1=NOTE_1 NAME_2=NOTE_2
    #[clap(value_parser, long="comment", short='c', num_args = 1..)]
    pub comments: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets upload <FILE_PATH> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct UploadSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    // NOTE: for now only accepts .env
    /// Path to file (dotenv format)
    pub file_path: String,

    /// Upload format
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsFileFormat>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets update -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct UpdateSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Values to update (format: NAME=NEW_VALUE). Use original name even if also renaming
    #[clap(value_parser, long = "value", short = 'v', num_args = 1..)]
    pub values: Vec<String>,

    /// Names to update (format: OLD_NAME=NEW_NAME). Use original name even if updating value
    #[clap(value_parser, long = "rename", short = 'r', num_args = 1..)]
    pub renames: Vec<String>,

    /// Comments to update (format: NAME=COMMENT). Use original name even if renaming
    #[clap(value_parser, long = "comment", short = 'c', num_args = 1..)]
    pub comments: Vec<String>,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq, Default)]
pub enum SecretsFileFormat {
    #[default]
    Dotenv,
    Yaml,
    Json,
}

impl Display for SecretsFileFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dotenv => write!(f, "dotenv"),
            Self::Json => write!(f, "json"),
            Self::Yaml => write!(f, "yaml"),
        }
    }
}

#[derive(Debug, Args)]
#[command(
    override_usage = "secrets comment <NAME> <COMMENT> -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]"
)]
pub struct SetComment {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secret name
    pub name: String,

    /// Comment
    pub comment: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets rename [SECETS] -p <PROJECT> -e <ENVIRONMENT> [OPTIONS]")]
pub struct RenameSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Secrets to rename: NAME_1=NEW_NAME_1 NAME_2=NEW_NAME_2
    #[clap(value_parser, num_args = 1..)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets search [OPTIONS]")]
pub struct SearchSecrets {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Secret name to search for
    #[arg(value_enum, long = "name", required = false)]
    pub name: Option<String>,

    /// Secret value to search for
    #[arg(value_enum, long = "value", required = false)]
    pub value: Option<String>,

    /// Reveal secret values, for search by name
    #[arg(value_enum, long = "show-values")]
    pub show_values: bool,

    /// Display also IDs of the projects and environments
    #[arg(value_enum, long = "with-ids")]
    pub with_ids: bool,

    /// Output format
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsSearchOutputFormat>,
}
