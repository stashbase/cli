use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AgentHooksCommand {
    #[clap(subcommand)]
    pub subcommand: Option<AgentHooksSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum AgentHooksSubcommand {
    /// Manage dependency hooks
    Deps(AgentDepsCommand),
}

#[derive(Debug, Args)]
pub struct AgentDepsCommand {
    #[clap(subcommand)]
    pub subcommand: AgentDepsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentDepsSubcommand {
    /// Install a dependency hook
    #[command(alias = "add")]
    Install(AgentHookTarget),

    /// Check whether a dependency hook is installed
    Check(AgentHookTarget),

    /// Remove a dependency hook
    #[command(alias = "remove")]
    Uninstall(AgentHookTarget),
}

#[derive(Debug, clap::Args)]
pub struct AgentHookTarget {
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
