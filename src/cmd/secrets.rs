use std::fmt::Display;

use clap::{Args, Subcommand, ValueEnum};

use crate::models::{
    scope::Scope, secrets::SecretsSearchOutputFormat, validation::InputValidationError,
};

use super::{
    config::SecretsOutputFormat,
    shared::{try_get_project_environment, try_get_scope, SharedProjectEnvArgs, SharedScopeArgs},
};

#[derive(Debug, Args)]
#[command(
    override_usage = "secrets <COMMAND> (-p <PROJECT> -e <ENVIRONMENT> | --scope=environment) [OPTIONS]"
)]
pub struct SecretArgs {
    /// Project name
    #[arg(short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,

    /// API scope [default: workspace]
    #[arg(long = "scope", value_enum)]
    pub scope: Option<Scope>,

    #[clap(subcommand)]
    pub subcommand: SecretSubcommand,
}

impl SecretArgs {
    pub fn try_get_project_environment(&self) -> Result<(String, String), InputValidationError> {
        let root_project: Option<_> = self.project.as_deref();
        let root_environment: Option<_> = self.environment.as_deref();

        let (project, environment) = self.subcommand.get_project_environment();

        try_get_project_environment(root_project, root_environment, project, environment)
    }

    pub fn get_scope(&self) -> Result<Scope, InputValidationError> {
        let root_scope = self.scope.as_ref();
        let subcommand_scope = self.subcommand.get_scope();

        // Check if scope is provided for commands that don't support it
        if subcommand_scope.is_none() && root_scope.is_some() {
            return Err(InputValidationError::CmdArgs(
                crate::models::validation::CmdArgInputValidationError::ScopeNotSupportedForCommand,
            ));
        }

        // For commands that don't support scope, return default workspace scope
        if subcommand_scope.is_none() {
            return Ok(Scope::Workspace);
        }

        // Use the validation function for commands that do support scope
        let resolved_scope = try_get_scope(root_scope, subcommand_scope)?;

        // If no scope provided, default to workspace
        Ok(resolved_scope.unwrap_or(Scope::Workspace))
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
            SecretSubcommand::Delete(d) => (
                d.shared_args.project.as_deref(),
                d.shared_args.environment.as_deref(),
            ),
            SecretSubcommand::Search(search_secrets) => {
                (Some(search_secrets.project.as_str()), None)
            }
            SecretSubcommand::Diff(diff_secrets) => (
                diff_secrets.shared_args.project.as_deref(),
                diff_secrets.shared_args.environment.as_deref(),
            ),
        }
    }
    pub fn get_scope(&self) -> Option<&Scope> {
        match self {
            SecretSubcommand::List(l) => l.scope_args.scope.as_ref(),
            SecretSubcommand::Get(g) => g.scope_args.scope.as_ref(),
            SecretSubcommand::Set(s) => s.scope_args.scope.as_ref(),
            SecretSubcommand::Create(c) => c.scope_args.scope.as_ref(),
            SecretSubcommand::Update(u) => u.scope_args.scope.as_ref(),
            SecretSubcommand::Upload(u) => u.scope_args.scope.as_ref(),
            SecretSubcommand::Delete(d) => d.scope_args.scope.as_ref(),
            // NOTE: diff and search commands don't support scope but its added for custom error handling
            SecretSubcommand::Diff(_) => None,
            SecretSubcommand::Search(_) => None,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum SecretSubcommand {
    /// List secrets
    List(ListSecrets),

    /// Get secrets
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
    Upload(UploadSecrets),

    // /// Rename secrets
    // #[clap(alias = "r")]
    // Rename(RenameSecrets),

    // /// Set comment of existing secret
    // #[clap(alias = "com")]
    // Comment(SetComment),
    /// Delete one or multiple secrets
    #[clap(aliases = &[ "del"])]
    Delete(DeleteSecrets),

    /// Search secrets by exact name or value
    Search(SearchSecrets),

    /// Compare local secrets with remote secrets
    Diff(DiffSecrets),
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets list [OPTIONS]")]
pub struct ListSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Output format
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsOutputFormat>,

    /// Print only names
    #[arg(long = "only-names")]
    pub only_names: bool,

    /// Expand references to their values
    #[arg(long = "expand-refs")]
    pub expand_refs: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets get <NAMES> [OPTIONS]")]
pub struct GetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    // #[clap(short='v', long="k", value_parser, num_args = 1.., value_delimiter = ' ')]
    pub names: Vec<String>,

    /// Format output
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsOutputFormat>,

    /// Expand references to their values
    #[arg(long = "expand-refs")]
    pub expand_refs: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets delete <NAMES> [OPTIONS]")]
pub struct DeleteSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Secrets (names) to delete
    #[clap(num_args = 1.., value_delimiter = ' ')]
    pub names: Vec<String>,

    /// Delete all secrets
    #[arg(name = "all", long = "all")]
    pub delete_all: bool,

    /// Proceed without confirmation
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets SET <SECRETS> [OPTIONS]")]
pub struct SetSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Secrets to set: NAME_1=VAL_1 NAME_2=VAL_2
    #[clap(num_args = 1..)]
    pub secrets: Vec<String>,

    /// Comments to set: NAME_1=NOTE_1 NAME_2=NOTE_2
    #[clap(long="comments", short='c', num_args = 1..)]
    pub comments: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets create <SECRETS> [OPTIONS]")]
pub struct CreateSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    /// Secrets to create: NAME_1=VAL_1 NAME_2=VAL_2
    #[clap(num_args = 1..)]
    pub secrets: Vec<String>,

    /// Comments to set: NAME_1=NOTE_1 NAME_2=NOTE_2
    #[clap(long="comments", short='c', num_args = 1..)]
    pub comments: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets upload <FILE_PATH> [OPTIONS]")]
pub struct UploadSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,

    // NOTE: for now only accepts .env
    /// Path to file (dotenv format)
    pub file_path: String,

    /// Upload format
    #[arg(short = 'f', long = "format")]
    pub format: Option<SecretsFileFormat>,

    /// Ignore secret comments
    #[arg(long = "ignore-comments")]
    pub ignore_comments: Option<bool>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets update [OPTIONS]")]
pub struct UpdateSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Values to update (format: NAME=NEW_VALUE). Use original name even if also renaming
    #[clap(long = "values", short = 'v', num_args = 1..)]
    pub values: Vec<String>,

    /// Names to update (format: OLD_NAME=NEW_NAME). Use original name even if updating value
    #[clap(long = "names", short = 'n', num_args = 1..)]
    pub new_names: Vec<String>,

    /// Comments to update (format: NAME=COMMENT). Use original name even if renaming
    #[clap(long = "comments", short = 'c', num_args = 1..)]
    pub comments: Vec<String>,

    #[clap(flatten)]
    pub scope_args: SharedScopeArgs,
}

#[derive(Debug, ValueEnum, Copy, Clone, PartialEq, Eq, Default)]
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
#[command(override_usage = "secrets search -p <PROJECT> [OPTIONS]")]
pub struct SearchSecrets {
    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = true)]
    pub project: String,

    /// Secret name to search for
    #[arg(value_enum, long = "name", required = false)]
    pub name: Option<String>,

    /// Secret value to search for
    #[arg(value_enum, long = "value", required = false)]
    pub value: Option<String>,

    /// Reveal secret values, for search by name
    #[arg(value_enum, long = "return-values")]
    pub return_values: bool,

    /// Output format
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<SecretsSearchOutputFormat>,

    #[arg(long = "scope", value_enum, hide = true, hide_long_help = true)]
    pub scope: Option<Scope>,
}

#[derive(Debug, Args)]
#[command(override_usage = "secrets diff <FILE_PATH> [OPTIONS]")]
pub struct DiffSecrets {
    #[clap(flatten)]
    pub shared_args: SharedProjectEnvArgs,

    /// Path to local secrets file
    pub file_path: String,

    /// Target file format (autodetected by default)
    #[arg(value_enum, long = "format")]
    pub format: Option<SecretsFileFormat>,

    /// Expand references to their values
    #[arg(value_enum, long = "expand-refs")]
    pub expand_refs: Option<bool>,

    /// Print and compare with comments
    #[arg(value_enum, long = "with-comments")]
    pub with_comments: bool,

    /// Show secret values
    #[arg(value_enum, long = "show-values")]
    pub show_values: bool,

    #[arg(long = "scope", value_enum, hide = true, hide_long_help = true)]
    pub scope: Option<Scope>,
}
