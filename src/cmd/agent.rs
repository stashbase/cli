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
    /// View local metadata-only broker audit logs
    Logs(AgentLogsCommand),
}

#[derive(Debug, Args)]
#[command(
    override_usage = "agent run --profile <PROFILE> [--profile-source <auto|global|directory>] -- <COMMAND> [ARGS]..."
)]
pub struct AgentRunCommand {
    /// Trusted agent profile from the Stashbase config file
    #[arg(long)]
    pub profile: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Temporarily trust the broker CA in the operating system trust store
    #[arg(long = "trust-broker-ca")]
    pub trust_broker_ca: bool,

    /// Experimental macOS network sandbox: only allows loopback access to the broker
    #[arg(long)]
    pub sandbox: bool,

    /// Bind the temporary broker to this localhost port instead of a random port
    #[arg(long)]
    pub broker_port: Option<u16>,

    /// Store metadata-only broker audit events locally
    #[arg(
        long,
        action = clap::ArgAction::Set,
        default_value_t = true,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub audit_log: bool,

    /// Command to run
    #[clap(num_args = 1..)]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct AgentLogsCommand {
    /// Number of most recent events to show
    #[arg(long, default_value_t = 50)]
    pub limit: usize,

    /// Only show events from this duration (for example: 30m, 24h, or 7d)
    #[arg(long)]
    pub since: Option<String>,

    /// Only show events for this agent profile
    #[arg(long)]
    pub profile: Option<String>,

    /// Only show events with this broker action (for example: injected)
    #[arg(long)]
    pub action: Option<String>,

    /// Only show events for this destination host
    #[arg(long)]
    pub host: Option<String>,

    /// Only show events for this broker session ID
    #[arg(long)]
    pub session: Option<String>,

    /// Keep watching for new events
    #[arg(long)]
    pub follow: bool,
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
