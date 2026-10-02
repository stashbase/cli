use serde::{Serialize, Serializer};
use uuid::Uuid;

use crate::{
    cmd::{
        agent::{AgentCommand, AgentSubcommand},
        root::EntityType,
    },
    models::{api_client::OutputError, validation::InputValidationError},
};

/// Flag *names* that may be reported, for non-agent commands only. Values are
/// never read. Every entry must be a real long flag (see the test that walks
/// the clap tree). Do not add flags whose names could carry user data.
pub const ALLOWED_FLAGS: &[&str] = &[
    "--json",
    "--silent",
    "--proxy",
    "--overwrite",
    "--only",
    "--exclude",
    "--scope",
    "--file",
    "--config",
    "--format",
    "--auth-check",
    "--verbose",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedCommand {
    Setup,
    Pull,
    Push,
    Run,
    Doctor,
    AgentInit,
    AgentRun,
    AgentDoctor,
    AgentValidate,
    AgentExplain,
    AgentPolicy,
    AgentMcp,
    AgentDocker,
    AgentWorktrees,
}

impl TrackedCommand {
    /// The command is chosen from the parsed clap enum, never from argv text.
    pub fn from_entity(entity: &EntityType) -> Option<Self> {
        match entity {
            EntityType::Setup(_) => Some(Self::Setup),
            EntityType::Pull(_) => Some(Self::Pull),
            EntityType::Push(_) => Some(Self::Push),
            EntityType::Run(_) => Some(Self::Run),
            EntityType::Doctor(_) => Some(Self::Doctor),
            // Exhaustive on purpose: a new `agent` subcommand must be classified
            // here as tracked or not before the crate compiles.
            EntityType::Agent(AgentCommand { subcommand }) => match subcommand {
                AgentSubcommand::Init(_) => Some(Self::AgentInit),
                AgentSubcommand::Run(_) => Some(Self::AgentRun),
                AgentSubcommand::Doctor(_) => Some(Self::AgentDoctor),
                AgentSubcommand::Validate(_) => Some(Self::AgentValidate),
                AgentSubcommand::Explain(_) => Some(Self::AgentExplain),
                AgentSubcommand::Policy(_) => Some(Self::AgentPolicy),
                AgentSubcommand::Mcp(_) => Some(Self::AgentMcp),
                AgentSubcommand::Docker(_) => Some(Self::AgentDocker),
                AgentSubcommand::Worktrees { .. } => Some(Self::AgentWorktrees),
                // Not tracked: `hooks` (its bare form is the broker that agent
                // tools call automatically, where a request would add latency
                // to every call), `sessions` and `profiles` (low signal), the
                // hidden MCP helpers, and local audit logs.
                AgentSubcommand::Hooks(_)
                | AgentSubcommand::Sessions { .. }
                | AgentSubcommand::Profiles(_)
                | AgentSubcommand::McpTools(_)
                | AgentSubcommand::McpCheck(_)
                | AgentSubcommand::Logs(_) => None,
            },
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Run => "run",
            Self::Doctor => "doctor",
            Self::AgentInit => "agent init",
            Self::AgentRun => "agent run",
            Self::AgentDoctor => "agent doctor",
            Self::AgentValidate => "agent validate",
            Self::AgentExplain => "agent explain",
            Self::AgentPolicy => "agent policy",
            Self::AgentMcp => "agent mcp",
            Self::AgentDocker => "agent docker",
            Self::AgentWorktrees => "agent worktrees",
        }
    }

    pub fn is_agent(self) -> bool {
        matches!(
            self,
            Self::AgentInit
                | Self::AgentRun
                | Self::AgentDoctor
                | Self::AgentValidate
                | Self::AgentExplain
                | Self::AgentPolicy
                | Self::AgentMcp
                | Self::AgentDocker
                | Self::AgentWorktrees
        )
    }
}

impl Serialize for TrackedCommand {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

/// Returns the allowlisted flag names present in `argv`, sorted and
/// de-duplicated. Scanning stops at `--`. Only the text before `=` is looked
/// at, and only to match it against `ALLOWED_FLAGS`; nothing from argv is
/// ever copied into the result.
pub fn collect_flags(command: TrackedCommand, argv: &[String]) -> Vec<&'static str> {
    if command.is_agent() {
        return Vec::new();
    }
    let mut found: Vec<&'static str> = Vec::new();
    for token in argv.iter().skip(1) {
        if token == "--" {
            break;
        }
        let name = token.split('=').next().unwrap_or("");
        if let Some(allowed) = ALLOWED_FLAGS.iter().find(|flag| **flag == name) {
            if !found.contains(allowed) {
                found.push(allowed);
            }
        }
    }
    found.sort_unstable();
    found
}

/// Which kind of profile an `agent run` loaded. Never the path or the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    Directory,
    Global,
    File,
}

/// Which sandbox backend an `agent run` used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxKind {
    Native,
    Docker,
}

/// Fixed funnel fields for `agent run`, present only once the run launched.
/// Counts are `None` (omitted) for remote runs, where policy is enforced
/// server-side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AgentRunInfo {
    pub profile_source: ProfileSource,
    pub remote: bool,
    pub sandbox_backend: SandboxKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_allow: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_deny: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_block: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Error,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Auth,
    Network,
    NotFound,
    Validation,
    Other,
}

pub fn outcome_for(
    exit_code: i32,
    aborted: bool,
    recorded: Option<ErrorKind>,
) -> (Outcome, Option<ErrorKind>) {
    if aborted || exit_code == 130 {
        (Outcome::Aborted, None)
    } else if recorded.is_some() || exit_code != 0 {
        (Outcome::Error, Some(recorded.unwrap_or(ErrorKind::Other)))
    } else {
        (Outcome::Ok, None)
    }
}

/// Maps an error to a coarse category by *type* only. The message is never
/// inspected, because it can contain paths, names or tokens.
pub fn classify_error(error: &anyhow::Error) -> ErrorKind {
    if error.downcast_ref::<reqwest::Error>().is_some()
        || error.downcast_ref::<reqwest_middleware::Error>().is_some()
    {
        return ErrorKind::Network;
    }
    if error.downcast_ref::<InputValidationError>().is_some() {
        return ErrorKind::Validation;
    }
    if let Some(output) = error.downcast_ref::<OutputError>() {
        return match output.get_status() {
            Some(401) | Some(403) => ErrorKind::Auth,
            Some(404) => ErrorKind::NotFound,
            _ => ErrorKind::Other,
        };
    }
    ErrorKind::Other
}

pub struct Invocation {
    pub command: TrackedCommand,
    pub flags: Vec<&'static str>,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub aborted: bool,
    pub recorded_error: Option<ErrorKind>,
    pub is_tty: bool,
    pub agent_run: Option<AgentRunInfo>,
}

/// The wire format. Only enums, integers, booleans, UUIDs and strings drawn
/// from fixed in-code lists. Keep `event_schema_is_pinned` in sync with any
/// change here, and document it in docs/telemetry.md.
#[derive(Debug, Serialize)]
pub struct Event {
    pub event: &'static str,
    pub command: TrackedCommand,
    pub flags: Vec<&'static str>,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ErrorKind>,
    pub duration_ms: u64,
    pub cli_version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub is_tty: bool,
    pub install_id: Uuid,
    pub event_id: Uuid,
    pub timestamp_ms: i64,
    #[serde(flatten)]
    pub agent_run: Option<AgentRunInfo>,
}

impl Event {
    pub fn new(invocation: Invocation, install_id: Uuid) -> Self {
        let (outcome, error_kind) = outcome_for(
            invocation.exit_code,
            invocation.aborted,
            invocation.recorded_error,
        );
        Self {
            event: "cli_command",
            command: invocation.command,
            flags: invocation.flags,
            outcome,
            error_kind,
            duration_ms: invocation.duration_ms,
            cli_version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            is_tty: invocation.is_tty,
            install_id,
            event_id: Uuid::now_v7(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            agent_run: invocation.agent_run,
        }
    }

    #[cfg(test)]
    pub fn sample() -> Event {
        Event::new(
            Invocation {
                command: TrackedCommand::Pull,
                flags: vec![],
                duration_ms: 1,
                exit_code: 0,
                aborted: false,
                recorded_error: None,
                is_tty: false,
                agent_run: None,
            },
            Uuid::new_v4(),
        )
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use uuid::Uuid;

    use super::*;
    use crate::cmd::root::Cli;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    fn invocation(command: TrackedCommand) -> Invocation {
        Invocation {
            command,
            flags: vec!["--json"],
            duration_ms: 12,
            exit_code: 1,
            aborted: false,
            recorded_error: Some(ErrorKind::Network),
            is_tty: true,
            agent_run: None,
        }
    }

    #[test]
    fn tracks_only_allowlisted_commands() {
        let tracked: Vec<(&[&str], &str)> = vec![
            (&["stashbase", "setup"], "setup"),
            (&["stashbase", "pull"], "pull"),
            (&["stashbase", "push"], "push"),
            (&["stashbase", "run", "--", "echo", "hi"], "run"),
            (&["stashbase", "doctor"], "doctor"),
            (&["stashbase", "agent", "init", "p"], "agent init"),
            (
                &["stashbase", "agent", "run", "--profile", "p", "--", "echo"],
                "agent run",
            ),
            (&["stashbase", "agent", "doctor", "curl"], "agent doctor"),
            (
                &["stashbase", "agent", "validate", "--profile", "p"],
                "agent validate",
            ),
            (
                &[
                    "stashbase", "agent", "explain", "--profile", "p", "--host", "h", "--method",
                    "GET", "--path", "/",
                ],
                "agent explain",
            ),
            (
                &["stashbase", "agent", "policy", "test", "--profile", "p"],
                "agent policy",
            ),
            (
                &[
                    "stashbase", "agent", "mcp", "configure", "--profile", "p", "--server", "s",
                ],
                "agent mcp",
            ),
            (&["stashbase", "agent", "docker", "status"], "agent docker"),
            (&["stashbase", "agent", "worktrees", "list"], "agent worktrees"),
        ];
        for (args, label) in tracked {
            let cli = parse(args);
            let command = TrackedCommand::from_entity(&cli.entity_type)
                .unwrap_or_else(|| panic!("{args:?} should be tracked"));
            assert_eq!(command.label(), label);
        }
    }

    #[test]
    fn untracked_commands_produce_no_command() {
        let untracked: Vec<&[&str]> = vec![
            &["stashbase", "whoami"],
            &["stashbase", "open"],
            &["stashbase", "secrets", "list"],
            &["stashbase", "config", "print"],
            &["stashbase", "scan", "install"],
            &["stashbase", "agent", "logs", "list"],
        ];
        for args in untracked {
            // Some of these may not parse without extra args; only assert on
            // the ones that do, so the test pins behaviour, not clap details.
            if let Ok(cli) = Cli::try_parse_from(args) {
                assert!(
                    TrackedCommand::from_entity(&cli.entity_type).is_none(),
                    "{args:?} must not be tracked"
                );
            }
        }
    }

    #[test]
    fn low_signal_and_automatic_agent_commands_are_not_tracked() {
        // `agent hooks` with no subcommand is the entry point agent tools
        // call automatically; a request there would add latency to every call.
        for args in [
            &["stashbase", "agent", "profiles", "list"][..],
            &["stashbase", "agent", "profiles", "show", "p"][..],
            &["stashbase", "agent", "sessions", "list"][..],
            &["stashbase", "agent", "hooks"][..],
            &["stashbase", "agent", "hooks", "deps", "check", "claude"][..],
            &["stashbase", "agent", "logs", "list"][..],
        ] {
            let cli = parse(args);
            assert!(
                TrackedCommand::from_entity(&cli.entity_type).is_none(),
                "{args:?} must not be tracked"
            );
        }
    }

    #[test]
    fn every_allowed_flag_exists_in_the_cli() {
        fn collect(cmd: &clap::Command, out: &mut Vec<String>) {
            for arg in cmd.get_arguments() {
                if let Some(long) = arg.get_long() {
                    out.push(format!("--{long}"));
                }
            }
            for sub in cmd.get_subcommands() {
                collect(sub, out);
            }
        }
        let mut known = Vec::new();
        collect(&Cli::command(), &mut known);
        for flag in ALLOWED_FLAGS {
            assert!(known.iter().any(|k| k == flag), "{flag} is not a real flag");
        }
    }

    #[test]
    fn flags_are_names_only_and_never_leak_values() {
        let args = argv(&[
            "stashbase",
            "push",
            "--api-key",
            "sk-live-SECRET",
            "--environment=prod-SECRET",
            "--only",
            "DB_PASSWORD_SECRET",
            "--set",
            "TOKEN=SECRET",
            "--json",
            "--",
            "--only",
            "after-separator",
        ]);
        let flags = collect_flags(TrackedCommand::Push, &args);
        assert_eq!(flags, vec!["--json", "--only"]);
        let joined = flags.join(" ");
        assert!(!joined.contains("SECRET"));
        assert!(!joined.contains("after-separator"));
    }

    #[test]
    fn agent_commands_never_report_flags() {
        let args = argv(&["stashbase", "agent", "run", "--profile", "p", "--json"]);
        assert!(collect_flags(TrackedCommand::AgentRun, &args).is_empty());
    }

    #[test]
    fn event_body_contains_no_sentinel_values() {
        let args = argv(&[
            "stashbase",
            "run",
            "--project",
            "SENTINEL_PROJECT",
            "--api-key",
            "SENTINEL_KEY",
            "--",
            "deploy",
            "--token",
            "SENTINEL_TOKEN",
            "/home/SENTINEL_PATH",
        ]);
        let cli = parse(&args.iter().map(String::as_str).collect::<Vec<_>>());
        let command = TrackedCommand::from_entity(&cli.entity_type).unwrap();
        let flags = collect_flags(command, &args);
        let event = Event::new(
            Invocation {
                command,
                flags,
                duration_ms: 5,
                exit_code: 1,
                aborted: false,
                recorded_error: Some(classify_error(&anyhow::anyhow!(
                    "failed for SENTINEL_PROJECT at /home/SENTINEL_PATH"
                ))),
                is_tty: false,
                agent_run: None,
            },
            Uuid::new_v4(),
        );
        let body = serde_json::to_string(&event).unwrap();
        assert!(!body.contains("SENTINEL"), "leaked into: {body}");
    }

    #[test]
    fn event_schema_is_pinned() {
        let event = Event::new(invocation(TrackedCommand::Pull), Uuid::new_v4());
        let value = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "arch",
                "cli_version",
                "command",
                "duration_ms",
                "error_kind",
                "event",
                "event_id",
                "flags",
                "install_id",
                "is_tty",
                "os",
                "outcome",
                "timestamp_ms",
            ],
            "changing the telemetry schema needs a deliberate review (docs/telemetry.md)"
        );
        // The extra `agent run` keys are pinned in
        // agent_run_fields_are_flattened_and_remote_runs_omit_counts.
        assert_eq!(value["event"], "cli_command");
        assert_eq!(value["command"], "pull");
        assert_eq!(value["outcome"], "error");
        assert_eq!(value["error_kind"], "network");
    }

    #[test]
    fn agent_run_fields_are_flattened_and_remote_runs_omit_counts() {
        let local = Event::new(
            Invocation {
                agent_run: Some(AgentRunInfo {
                    profile_source: ProfileSource::Directory,
                    remote: false,
                    sandbox_backend: SandboxKind::Docker,
                    policy_allow: Some(7),
                    policy_deny: Some(2),
                    policy_block: Some(0),
                }),
                ..invocation(TrackedCommand::AgentRun)
            },
            Uuid::new_v4(),
        );
        let value = serde_json::to_value(&local).unwrap();
        assert_eq!(value["profile_source"], "directory");
        assert_eq!(value["remote"], false);
        assert_eq!(value["sandbox_backend"], "docker");
        assert_eq!(value["policy_allow"], 7);
        assert_eq!(value["policy_deny"], 2);
        assert_eq!(value["policy_block"], 0);

        let remote = Event::new(
            Invocation {
                agent_run: Some(AgentRunInfo {
                    profile_source: ProfileSource::Global,
                    remote: true,
                    sandbox_backend: SandboxKind::Native,
                    policy_allow: None,
                    policy_deny: None,
                    policy_block: None,
                }),
                ..invocation(TrackedCommand::AgentRun)
            },
            Uuid::new_v4(),
        );
        let value = serde_json::to_value(&remote).unwrap();
        assert_eq!(value["profile_source"], "global");
        assert_eq!(value["sandbox_backend"], "native");
        assert!(value.get("policy_allow").is_none());
        assert!(value.get("policy_deny").is_none());
        assert!(value.get("policy_block").is_none());

        // A run that failed before launching carries none of the funnel fields.
        let failed_early = serde_json::to_value(Event::new(
            invocation(TrackedCommand::AgentRun),
            Uuid::new_v4(),
        ))
        .unwrap();
        for key in ["profile_source", "remote", "sandbox_backend", "policy_allow"] {
            assert!(failed_early.get(key).is_none(), "{key}");
        }
    }

    #[test]
    fn outcome_mapping() {
        assert_eq!(outcome_for(0, false, None), (Outcome::Ok, None));
        assert_eq!(
            outcome_for(1, false, None),
            (Outcome::Error, Some(ErrorKind::Other))
        );
        // Many handlers print an error and return normally (exit code 0).
        assert_eq!(
            outcome_for(0, false, Some(ErrorKind::Auth)),
            (Outcome::Error, Some(ErrorKind::Auth))
        );
        assert_eq!(outcome_for(130, false, None), (Outcome::Aborted, None));
        assert_eq!(outcome_for(0, true, None), (Outcome::Aborted, None));
    }

    #[test]
    fn classify_error_never_depends_on_message_text() {
        let plain = anyhow::anyhow!("401 unauthorized for /home/user/.secret");
        assert_eq!(classify_error(&plain), ErrorKind::Other);
        let validation = anyhow::anyhow!(crate::models::validation::InputValidationError::MissingApiKey);
        assert_eq!(classify_error(&validation), ErrorKind::Validation);
    }
}
