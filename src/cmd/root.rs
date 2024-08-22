use clap::{Parser, Subcommand};

use super::{
    config::ConfigCommand, environments::EnvironmentCommands, projects::ProjectCommands,
    pull::PullCommand, push::PushCommand, run::RunCommand, scan::ScanCommand, secrets::SecretArgs,
    webhooks::WebhookCommand,
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

    // /// Output data as raw json
    // #[arg(long = "raw", global = true)]
    // pub raw: bool,

    // #[clap(long, value_enum, global = true, default_value = "auto")]
    // pub color: Color,
    //
    #[clap(subcommand)]
    pub entity_type: EntityType,
}

// #[derive(ValueEnum, Clone, Copy, Debug)]
// pub enum Color {
//     Auto,
//     Never,
// }
//
// impl Color {
//     pub fn init(self) {
//         // Set a supports-color override based on the variable passed in.
//         match self {
//             Color::Auto => {}
//             Color::Never => owo_colors::set_override(false),
//         }
//     }
// }
//
#[derive(Debug, Subcommand)]
pub enum EntityType {
    /// Load environment and run command
    Run(RunCommand),

    /// Pull secrets from environment
    Pull(PullCommand),

    /// Push secrets to environment
    Push(PushCommand),

    /// Scan files for secrets
    Scan(ScanCommand),

    #[clap(name = "projects", aliases = &["p", "pro", "proj"])]
    /// Manage projects
    Project(ProjectCommands),

    /// Manage environments
    #[clap(name = "environments", aliases = &["e", "env"])]
    Environment(EnvironmentCommands),

    /// Manage secrets
    #[clap(name = "secrets", aliases = &["s", "sec"])]
    Secret(SecretArgs),

    /// Manage webhooks
    #[clap(name = "webhooks", aliases = &["w", "web"])]
    Webhooks(WebhookCommand),

    /// Your CLI configuration
    #[clap(aliases = &["c", "conf"])]
    Config(ConfigCommand),

    /// Open web dashboard
    Open,
}
