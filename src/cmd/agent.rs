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

    /// Command to run
    #[clap(num_args = 1..)]
    pub command: Vec<String>,
}
