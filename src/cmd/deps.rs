use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AgentHooksCommand {
    #[clap(subcommand)]
    pub subcommand: Option<AgentHooksSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum AgentHooksSubcommand {
    /// Install an agent dependency hook
    #[command(alias = "add")]
    Install(AgentHookInstall),

    /// Check whether an agent dependency hook is installed
    Check(AgentHookInstall),

    /// Remove an agent dependency hook
    #[command(alias = "uninstall")]
    Remove(AgentHookInstall),
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum AgentHook {
    #[value(alias = "dependencies")]
    Deps,
}

#[derive(Debug, clap::Args)]
pub struct AgentHookInstall {
    /// Hook to install, check, or remove
    #[arg(value_enum)]
    pub hook: AgentHook,

    /// Agent configuration to inspect or modify
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
    Cursor,
}
