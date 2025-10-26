use clap::{Parser, Subcommand, ValueEnum};

use crate::cmd::scans::ScanCommands;

use super::{
    config::ConfigCommand, environments::EnvironmentCommands, generate::GenerateCommand,
    projects::ProjectCommands, pull::PullCommand, push::PushCommand, run::RunCommand,
    secrets::SecretArgs, setup::SetupCommand, webhooks::WebhookCommand,
};

#[derive(Debug, Parser)]
#[command(author, version)]
#[command(about = "Stashbase CLI")]
#[command(override_usage = "stashbase <COMMAND> [OPTIONS]")]

pub struct Cli {
    /// Output data as pretty json
    #[arg(long = "json", name = "json", global = true)]
    pub raw: bool,

    /// Manualy set API key for the command
    #[arg(long = "api-key", global = true)]
    pub api_key: Option<String>,

    /// Suppress non-essential output
    #[arg(long = "silent", global = true)]
    pub silent: bool,

    /// When to use colored output
    #[arg(long, value_enum, global = true, default_value = "auto")]
    pub color: ColorChoice,

    #[clap(subcommand)]
    pub entity_type: EntityType,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ColorChoice {
    /// Automatically detect if colors should be used
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

#[derive(Debug, Subcommand)]
pub enum EntityType {
    /// Load environment and run command
    Run(RunCommand),

    /// Pull secrets from environment
    Pull(PullCommand),

    /// Push secrets to environment
    Push(PushCommand),

    #[clap(name = "projects", aliases = &["proj"])]
    /// Manage projects
    Project(ProjectCommands),

    /// Manage environments
    #[clap(name = "environments", aliases = &["env"])]
    Environment(EnvironmentCommands),

    /// Manage secrets
    #[clap(name = "secrets", aliases = &["sec"])]
    Secret(SecretArgs),

    /// Scan for hardcoded secrets
    #[clap(name = "scan")]
    Scan(ScanCommands),

    /// Manage webhooks
    #[clap(name = "webhooks", aliases = &["web"])]
    Webhooks(WebhookCommand),

    /// Generate random string or UUID
    #[clap(name = "generate", aliases = &["gen"])]
    Generate(GenerateCommand),

    /// Your CLI configuration
    Config(ConfigCommand),

    /// Interactively configure Stashbase CLI
    Setup(SetupCommand),

    /// Open web dashboard
    #[clap(name = "open", aliases = &["dashboard"])]
    Open,

    /// Show details of currently authenticated entity
    #[clap(name = "whoami", aliases = &["me"])]
    Whoami,
}

impl EntityType {
    pub fn requires_api_key(&self) -> bool {
        match self {
            EntityType::Generate(_) => false,
            EntityType::Config(_) => false,
            _ => true,
        }
    }
}
