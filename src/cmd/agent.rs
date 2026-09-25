use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::deps::AgentHooksCommand;

#[derive(Debug, Args)]
pub struct AgentCommand {
    #[command(subcommand)]
    pub subcommand: AgentSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentSubcommand {
    /// Manage agent hooks
    Hooks(AgentHooksCommand),
    /// Create a safe starter profile in .stashbase/agents
    Init(AgentInitCommand),
    /// Run an agent through the Stashbase Agent Proxy
    Run(AgentRunCommand),
    /// List active agent sessions
    Sessions {
        #[command(subcommand)]
        command: AgentSessionsSubcommand,
    },
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
    /// Manage Docker sandbox backend resources
    Docker(AgentDockerCommand),
}

#[derive(Debug, Subcommand)]
pub enum AgentSessionsSubcommand {
    /// List active agent sessions
    List(AgentSessionsCommand),
    /// Revoke an active local or remote agent session
    Revoke(AgentRevokeCommand),
}

#[derive(Debug, Args)]
pub struct AgentDockerCommand {
    #[command(subcommand)]
    pub subcommand: AgentDockerSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentDockerSubcommand {
    /// Find and remove Docker sandbox networks/containers left behind by a run that didn't tear down cleanly (e.g. `stashbase` was killed with SIGKILL mid-run)
    Cleanup(AgentDockerCleanupCommand),
    /// List Docker sandbox networks/containers currently present on this machine
    Status(AgentDockerStatusCommand),
    /// Build (or rebuild) the default Docker sandbox image
    Build(AgentDockerBuildCommand),
    /// Check whether the Docker sandbox backend can run on this machine
    Doctor(AgentDockerDoctorCommand),
}

#[derive(Debug, Args)]
pub struct AgentDockerDoctorCommand {}

#[derive(Debug, Args)]
pub struct AgentDockerCleanupCommand {
    /// Remove every leftover resource without prompting for confirmation
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct AgentDockerStatusCommand {}

#[derive(Debug, Args)]
pub struct AgentDockerBuildCommand {
    /// Rebuild even if the image already exists locally
    #[arg(long)]
    pub force: bool,

    /// Build the given profile's `sandbox.image`/`sandbox.dockerfile` instead of the built-in default image
    #[arg(long)]
    pub profile: Option<String>,

    /// Where to load --profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,
}

#[derive(Debug, Args)]
#[command(override_usage = "agent sessions list [--local | --remote]")]
pub struct AgentSessionsCommand {
    /// Show only sessions running on this machine
    #[arg(long, conflicts_with = "remote")]
    pub local: bool,
    /// Show only sessions on the Stashbase Agent Proxy
    #[arg(long, conflicts_with = "local")]
    pub remote: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "agent sessions revoke [<SESSION_ID> | --all] [--local | --remote]")]
pub struct AgentRevokeCommand {
    /// Session ID from `agent sessions`
    #[arg(required_unless_present = "all", conflicts_with = "all")]
    pub session_id: Option<String>,
    /// Revoke all active sessions in the selected scope
    #[arg(long)]
    pub all: bool,
    /// Revoke a local session only
    #[arg(long, conflicts_with = "remote")]
    pub local: bool,
    /// Revoke a remote session only
    #[arg(long, conflicts_with = "local")]
    pub remote: bool,
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
    /// Select and save the tools allowed for a configured HTTP MCP server
    Configure(AgentMcpConfigureCommand),
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

    /// Bind the temporary proxy to this localhost port instead of a random port
    #[arg(long)]
    pub proxy_port: Option<u16>,

    /// Resolve Stashbase secrets in a short-lived remote agent proxy session
    #[arg(long)]
    pub remote: bool,

    /// Override the profile's `[sandbox] backend` for this run only: `true`
    /// forces the Docker backend, `false` forces the native backend.
    /// Omit to use whatever the profile declares.
    #[arg(long, value_parser = clap::builder::BoolishValueParser::new())]
    pub docker_sandbox: Option<bool>,

    /// Override the profile's `[sandbox] image` for this run only: run this
    /// image instead of the profile's configured one (or the built-in
    /// default). Implies the Docker backend even if the profile or
    /// `--docker-sandbox` says otherwise. Mutually exclusive with
    /// `--docker-dockerfile`.
    #[arg(long, conflicts_with = "docker_dockerfile")]
    pub docker_image: Option<String>,

    /// Override the profile's `[sandbox] dockerfile` for this run only:
    /// build and run this Dockerfile instead of the profile's configured
    /// one (or the built-in default). Implies the Docker backend even if
    /// the profile or `--docker-sandbox` says otherwise. Mutually
    /// exclusive with `--docker-image`.
    #[arg(long, conflicts_with = "docker_image")]
    pub docker_dockerfile: Option<String>,

    /// Override the profile's `[sandbox] memory` for this run only:
    /// `docker run --memory` value, e.g. "2g". No cap by default.
    #[arg(long)]
    pub docker_memory: Option<String>,

    /// Override the profile's `[sandbox] cpus` for this run only: `docker
    /// run --cpus` value, e.g. "1.5". No cap by default.
    #[arg(long)]
    pub docker_cpus: Option<String>,

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
pub struct AgentMcpConfigureCommand {
    /// Agent profile containing the MCP server configuration
    #[arg(long)]
    pub profile: String,

    /// Named entry under [mcp_servers]
    #[arg(long)]
    pub server: String,

    /// Where to load the agent profile from
    #[arg(long, value_enum, default_value = "auto")]
    pub profile_source: AgentProfileSource,

    /// Explicit writable profile file
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

    #[test]
    fn sessions_command_supports_all_local_and_remote_filters() {
        use crate::cmd::root::Cli;
        use clap::Parser;

        assert!(Cli::try_parse_from(["stashbase", "agent", "sessions", "list"]).is_ok());
        assert!(Cli::try_parse_from(["stashbase", "agent", "sessions", "list", "--local"]).is_ok());
        assert!(
            Cli::try_parse_from(["stashbase", "agent", "sessions", "list", "--remote"]).is_ok()
        );
        assert!(Cli::try_parse_from([
            "stashbase",
            "agent",
            "sessions",
            "list",
            "--local",
            "--remote"
        ])
        .is_err());
        assert!(
            Cli::try_parse_from(["stashbase", "agent", "sessions", "revoke", "ags_test"]).is_ok()
        );
        assert!(Cli::try_parse_from(["stashbase", "agent", "sessions", "revoke", "--all"]).is_ok());
        assert!(Cli::try_parse_from([
            "stashbase",
            "agent",
            "sessions",
            "revoke",
            "--all",
            "--local"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "stashbase",
            "agent",
            "sessions",
            "revoke",
            "ags_test",
            "--all"
        ])
        .is_err());
        let local =
            Cli::try_parse_from(["stashbase", "agent", "sessions", "list", "--local"]).unwrap();
        let remote =
            Cli::try_parse_from(["stashbase", "agent", "sessions", "list", "--remote"]).unwrap();
        let all = Cli::try_parse_from(["stashbase", "agent", "sessions", "list"]).unwrap();
        let local_revoke = Cli::try_parse_from([
            "stashbase",
            "agent",
            "sessions",
            "revoke",
            "ags_test",
            "--local",
        ])
        .unwrap();
        let remote_revoke =
            Cli::try_parse_from(["stashbase", "agent", "sessions", "revoke", "ags_test"]).unwrap();
        assert!(!local.entity_type.requires_api_key());
        assert!(remote.entity_type.requires_api_key());
        assert!(all.entity_type.requires_api_key());
        assert!(!local_revoke.entity_type.requires_api_key());
        assert!(remote_revoke.entity_type.requires_api_key());
    }
}
