use clap::{Parser, Subcommand};

use super::{
    configs::ConfigCommands, environments::EnvironmentCommands, projects::ProjectCommands,
    run::RunCommand, secrets::SecretArgs,
};

#[derive(Debug, Parser)]
#[command(author, version)]
#[command(about = "Env ease CLI")]

pub struct Cli {
    /// Output data as pretty json
    #[arg(long = "json", name = "json", global = true)]
    pub raw: bool,

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

    #[clap(name = "projects", aliases = &["p", "pro", "proj"])]
    /// Manage projects
    Project(ProjectCommands),

    /// Manage environments
    #[clap(name = "environments", aliases = &["e", "env"])]
    Environment(EnvironmentCommands),

    /// Manage secrets
    #[clap(name = "secrets", aliases = &["s", "sec"])]
    Secret(SecretArgs),

    /// Your CLI configuration
    #[clap(aliases = &["c", "conf"])]
    Config(ConfigCommands),

    Open,
}
