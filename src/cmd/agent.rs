use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
pub struct AgentCommand {
    #[command(subcommand)]
    pub subcommand: AgentSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentSubcommand {
    /// Create a safe starter profile in .stashbase/agents
    Init(AgentInitCommand),
    /// Run an agent through the Stashbase Agent Proxy
    Run(AgentRunCommand),
    /// Validate an agent profile without loading secrets or starting a proxy
    Validate(AgentValidateCommand),
    /// Explain how an agent profile would handle an HTTP request without loading secrets
    Explain(AgentExplainCommand),
    /// Run local, declarative policy regression tests without loading secrets or making requests
    Policy(AgentPolicyCommand),
    /// List and inspect available agent profiles without loading secrets
    Profiles(AgentProfilesCommand),
    /// Check a tool's compatibility with the temporary Agent Proxy
    Doctor(AgentDoctorCommand),
    /// Inspect and evaluate configured HTTP MCP servers
    Mcp(AgentMcpCommand),
    /// Inspect the tools exposed by a configured HTTP MCP server
    #[command(hide = true)]
    McpTools(AgentMcpToolsCommand),
    /// Check whether one MCP tool is allowed by the configured policy
    #[command(hide = true)]
    McpCheck(AgentMcpCheckCommand),
    /// View local metadata-only proxy audit logs
    Logs(AgentLogsCommand),
}

#[derive(Debug, Args)]
pub struct AgentMcpCommand {
    #[command(subcommand)]
    pub subcommand: AgentMcpSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentMcpSubcommand {
    /// Inspect the tools exposed by a configured HTTP MCP server
    Tools(AgentMcpToolsCommand),
    /// Check whether one MCP tool is allowed by the configured policy
    Check(AgentMcpCheckCommand),
    /// Verify configured MCP tool names against the server's tools/list response
    Verify(AgentMcpVerifyCommand),
}

#[derive(Debug, Args)]
#[command(override_usage = "agent init <PROFILE> [--force]")]
pub struct AgentInitCommand {
    /// Name for the new repository-local profile
    pub profile: String,

    /// Replace an existing profile file
    #[arg(long)]
    pub force: bool,
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

    /// Show resolved defaults and normalized policy values instead of raw TOML
    #[arg(long)]
    pub effective: bool,
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
    override_usage = "agent mcp-tools --profile <PROFILE> --server <SERVER> [--profile-source <auto|global|directory>]"
)]
pub struct AgentMcpToolsCommand {
    /// Agent profile containing the MCP server configuration
    #[arg(long)]
    pub profile: String,

    /// Named entry under [mcp_servers]
    #[arg(long)]
    pub server: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Explicit direct profile file
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,

    /// Resolve MCP bindings through a short-lived remote Agent Proxy session
    #[arg(long)]
    pub remote: bool,
}

#[derive(Debug, Args)]
pub struct AgentMcpCheckCommand {
    #[arg(long)]
    pub profile: String,
    #[arg(long)]
    pub server: String,
    #[arg(long)]
    pub tool: String,
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct AgentMcpVerifyCommand {
    #[arg(long)]
    pub profile: String,
    #[arg(long)]
    pub server: String,
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,

    /// Resolve MCP bindings through a short-lived remote Agent Proxy session
    #[arg(long)]
    pub remote: bool,
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

    /// Show normalized request details and the matching HTTP rule number
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct AgentPolicyCommand {
    #[command(subcommand)]
    pub subcommand: AgentPolicySubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentPolicySubcommand {
    /// Verify declarative HTTP policy cases without loading secrets or making requests
    Test(AgentPolicyTestCommand),
}

#[derive(Debug, Args)]
#[command(
    override_usage = "agent policy test --profile <PROFILE> [--test-file <PATH>] [--profile-source <auto|global|directory>]"
)]
pub struct AgentPolicyTestCommand {
    /// Agent profile to test
    #[arg(long)]
    pub profile: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Explicit direct profile file; bypasses global and directory lookup
    #[arg(long, conflicts_with = "profile_source")]
    pub policy_file: Option<PathBuf>,

    /// TOML policy test file (defaults to .stashbase/agent-policy-tests.toml)
    #[arg(long)]
    pub test_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct AgentLogsCommand {
    #[command(subcommand)]
    pub subcommand: Option<AgentLogsSubcommand>,

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

#[derive(Debug, Subcommand)]
pub enum AgentLogsSubcommand {
    /// List individual local proxy audit events
    List(AgentLogsListCommand),
    /// Summarize recent proxy outcomes and denied destinations
    Summary(AgentLogsSummaryCommand),
}

#[derive(Debug, Args)]
pub struct AgentLogsListCommand {
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

#[derive(Debug, Args)]
pub struct AgentLogsSummaryCommand {
    /// Number of most recent events to include
    #[arg(long, default_value_t = 1_000)]
    pub limit: usize,

    /// Only include events from this duration (for example: 30m, 24h, or 7d)
    #[arg(long)]
    pub since: Option<String>,

    /// Only include events for this agent profile
    #[arg(long)]
    pub profile: Option<String>,

    /// Only include events with this proxy action
    #[arg(long)]
    pub action: Option<String>,

    /// Only include events for this destination host
    #[arg(long)]
    pub host: Option<String>,

    /// Only include events for this proxy session ID
    #[arg(long)]
    pub session: Option<String>,

    /// Only include one local audit event by ID
    #[arg(long)]
    pub id: Option<String>,

    /// Group matching events by host, proxy action, or credential binding
    #[arg(long = "by", value_enum)]
    pub group_by: Option<AgentAuditGroupBy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentAuditGroupBy {
    /// Group by destination host
    Host,
    /// Group by proxy action
    Action,
    /// Group by configured credential binding name
    #[value(alias = "secret")]
    Binding,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AgentProfileSource {
    /// User-level Stashbase config
    Global,
    /// .stashbase/agents/<profile>.toml in the current directory
    Directory,
    /// Directory profile when present, otherwise user-level config
    Auto,
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum;

    use super::AgentAuditGroupBy;

    #[test]
    fn audit_binding_group_accepts_the_legacy_secret_alias() {
        assert_eq!(
            AgentAuditGroupBy::from_str("binding", true),
            Ok(AgentAuditGroupBy::Binding)
        );
        assert_eq!(
            AgentAuditGroupBy::from_str("secret", true),
            Ok(AgentAuditGroupBy::Binding)
        );
    }
}
