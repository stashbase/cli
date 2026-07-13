use clap::{Args, Subcommand};

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
#[command(override_usage = "agent run --profile <PROFILE> -- <COMMAND> [ARGS]...")]
pub struct AgentRunCommand {
    /// Trusted agent profile from the Stashbase config file
    #[arg(long)]
    pub profile: String,

    /// Temporarily trust the broker CA in the operating system trust store
    #[arg(long = "trust-broker-ca")]
    pub trust_broker_ca: bool,

    /// Command to run
    #[clap(num_args = 1..)]
    pub command: Vec<String>,
}
