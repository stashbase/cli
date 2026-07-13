use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
pub struct AgentCommand {
    #[command(subcommand)]
    pub subcommand: AgentSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentSubcommand {
    /// Run an agent with a brokered credential profile
    Run(AgentRunCommand),
}

#[derive(Debug, Args)]
#[command(
    override_usage = "agent run --profile <PROFILE> [--profile-source <global|directory|auto>] -- <COMMAND> [ARGS]..."
)]
pub struct AgentRunCommand {
    /// Trusted agent profile from the Stashbase config file
    #[arg(long)]
    pub profile: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "global")]
    pub profile_source: AgentProfileSource,

    /// Temporarily trust the broker CA in the operating system trust store
    #[arg(long = "trust-broker-ca")]
    pub trust_broker_ca: bool,

    /// Experimental macOS network sandbox: only allows loopback access to the broker
    #[arg(long)]
    pub sandbox: bool,

    /// Command to run
    #[clap(num_args = 1..)]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AgentProfileSource {
    /// User-level Stashbase config
    Global,
    /// .stashbase.toml in the current directory
    Directory,
    /// Directory config when present, otherwise user-level config
    Auto,
}
