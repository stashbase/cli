use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
pub struct AgentCommand {
    #[command(subcommand)]
    pub subcommand: AgentSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentSubcommand {
    /// Run an agent through the Stashbase Agent Proxy
    Run(AgentRunCommand),
    /// Validate an agent profile without loading secrets or starting a proxy
    Validate(AgentValidateCommand),
    /// Explain how an agent profile would handle an HTTP request without loading secrets
    Explain(AgentExplainCommand),
    /// List and inspect available agent profiles without loading secrets
    Profiles(AgentProfilesCommand),
    /// Check a tool's compatibility with the temporary Agent Proxy
    Doctor(AgentDoctorCommand),
    /// View local metadata-only proxy audit logs
    Logs(AgentLogsCommand),
}

#[derive(Debug, Args)]
pub struct AgentProfilesCommand {
    #[command(subcommand)]
    pub subcommand: AgentProfilesSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentProfilesSubcommand {
    /// List available profiles and their selected source
    List(AgentProfilesListCommand),
    /// Show one profile without loading secret values
    Show(AgentProfilesShowCommand),
}

#[derive(Debug, Args)]
pub struct AgentProfilesListCommand {
    /// Which profile sources to include
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,
}

#[derive(Debug, Args)]
pub struct AgentProfilesShowCommand {
    /// Profile name to display
    pub profile: String,

    /// Where to load the profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Explicit direct profile file; bypasses global and directory lookup
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,
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

    /// Explicit direct profile file; bypasses global and directory lookup
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,

    /// Temporarily trust the proxy CA in the operating system trust store
    #[arg(long)]
    pub trust_proxy_ca: bool,

    /// Experimental network sandbox: only allows loopback access to the proxy
    #[arg(long)]
    pub sandbox: bool,

    /// Bind the temporary proxy to this localhost port instead of a random port
    #[arg(long)]
    pub proxy_port: Option<u16>,

    /// Resolve Stashbase secrets in a short-lived remote agent proxy session
    #[arg(long)]
    pub remote: bool,

    /// Store metadata-only proxy audit events locally
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
#[command(override_usage = "agent doctor [--remote] <TOOL>")]
pub struct AgentDoctorCommand {
    /// Also verify the remote Agent Proxy CA required by --remote sessions
    #[arg(long)]
    pub remote: bool,

    /// Executable to check (for example: curl, gh, node, copilot, or codex)
    pub tool: String,
}

#[derive(Debug, Args)]
#[command(
    override_usage = "agent validate --profile <PROFILE> [--profile-source <auto|global|directory>] [--remote]"
)]
pub struct AgentValidateCommand {
    /// Agent profile to validate
    #[arg(long)]
    pub profile: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Explicit direct profile file; bypasses global and directory lookup
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,

    /// Also verify requirements for a --remote agent session
    #[arg(long)]
    pub remote: bool,
}

#[derive(Debug, Args)]
#[command(
    override_usage = "agent explain --profile <PROFILE> --host <HOST> --method <METHOD> --path <PATH> [--profile-source <auto|global|directory>]"
)]
pub struct AgentExplainCommand {
    /// Agent profile to evaluate
    #[arg(long)]
    pub profile: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Explicit direct profile file; bypasses global and directory lookup
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,

    /// Destination hostname to evaluate
    #[arg(long)]
    pub host: String,

    /// HTTP method to evaluate
    #[arg(long)]
    pub method: String,

    /// URL path to evaluate; query strings are ignored
    #[arg(long)]
    pub path: String,
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

    /// Only show events with this proxy action (for example: injected)
    #[arg(long)]
    pub action: Option<String>,

    /// Only show events for this destination host
    #[arg(long)]
    pub host: Option<String>,

    /// Only show events for this proxy session ID
    #[arg(long)]
    pub session: Option<String>,

    /// Only show one local audit event by ID
    #[arg(long)]
    pub id: Option<String>,

    /// Keep watching for new events
    #[arg(long)]
    pub follow: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AgentProfileSource {
    /// User-level Stashbase config
    Global,
    /// .stashbase/agents/<profile>.toml or legacy stashbase-agent.toml in the current directory
    Directory,
    /// Directory profile when present, otherwise user-level config
    Auto,
}
