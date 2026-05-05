use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::cmd::scans::{ScanCommands, ScanSubcommand};

use super::{
    config::ConfigCommand, doctor::DoctorCommand, environments::EnvironmentCommands,
    generate::GenerateCommand, projects::ProjectCommands, pull::PullCommand, push::PushCommand,
    run::RunCommand, secrets::SecretArgs, setup::SetupCommand, webhooks::WebhookCommand,
};

#[derive(Debug, Parser)]
#[command(author, version)]
#[command(about = "Stashbase CLI")]
#[command(override_usage = "stashbase <COMMAND> [OPTIONS]")]

pub struct Cli {
    /// Output data as pretty json
    #[arg(long = "json", name = "json", global = true)]
    pub raw: bool,

    /// Manually set API key for the command
    #[arg(long = "api-key", global = true)]
    pub api_key: Option<String>,

    /// Suppress non-essential output
    #[arg(long = "silent", global = true)]
    pub silent: bool,

    /// When to use colored output
    #[arg(long, value_enum, global = true, default_value = "auto")]
    pub color: ColorChoice,

    /// HTTP request timeout in seconds
    #[arg(long = "timeout", global = true, value_parser = clap::value_parser!(u64).range(1..=600))]
    pub timeout: Option<u64>,

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

    /// Generate random string, UUID, hash, passphrase, or SSH key pair
    #[clap(name = "generate", aliases = &["gen"])]
    Generate(GenerateCommand),

    /// Diagnose CLI configuration and environment
    #[clap(name = "doctor", aliases = &["diag", "doc", "diagnose"])]
    Doctor(DoctorCommand),

    /// Your CLI configuration
    Config(ConfigCommand),

    /// Interactively configure Stashbase CLI
    Setup(SetupCommand),

    /// Open web dashboard
    #[clap(name = "open", aliases = &["dashboard"])]
    Open,

    /// Show details of currently authenticated entity
    #[clap(name = "whoami", aliases = &["me"])]
    Whoami(WhoamiCommand),
}

impl EntityType {
    pub fn requires_api_key(&self) -> bool {
        match self {
            EntityType::Generate(_) => false,
            EntityType::Config(_) => false,
            EntityType::Doctor(_) => false,
            EntityType::Scan(scan_cmd) => !matches!(
                scan_cmd.subcommand,
                ScanSubcommand::Install(_) | ScanSubcommand::Uninstall(_)
            ),
            _ => true,
        }
    }
}

#[derive(Debug, Args)]
#[command(override_usage = "whoami [OPTIONS]")]
pub struct WhoamiCommand {
    /// Output format
    #[arg(value_enum, short = 'f', long = "format")]
    pub format: Option<WhoamiOutputFormat>,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
pub enum WhoamiOutputFormat {
    List,
    Table,
    Json,
}
