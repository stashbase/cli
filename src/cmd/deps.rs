use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct DepsCommands {
    #[clap(subcommand)]
    pub subcommand: DepsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DepsSubcommand {
    /// Check dependencies from an agent command hook
    Hook(HookCommands),
}

#[derive(Debug, Args)]
pub struct HookCommands {
    #[clap(subcommand)]
    pub subcommand: Option<HookSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum HookSubcommand {
    /// Install the dependency hook for an agent in this repository
    Install(HookInstall),

    /// Remove the dependency hook from an agent configuration
    #[command(alias = "remove")]
    Uninstall(HookInstall),
}

#[derive(Debug, clap::Args)]
pub struct HookInstall {
    /// Agent configuration to modify
    #[arg(value_enum)]
    pub agent: HookAgent,

    /// Apply to the global configuration instead of only this repository
    #[arg(long)]
    pub global: bool,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum HookAgent {
    Claude,
    Codex,
}
