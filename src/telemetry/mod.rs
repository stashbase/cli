pub mod consent;
pub mod event;
pub mod send;
pub mod state;

use std::{
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Mutex, MutexGuard,
    },
    time::Instant,
};

use once_cell::sync::Lazy;
use uuid::Uuid;

use crate::cmd::root::EntityType;
use consent::{Decision, Signals};
use event::{
    AgentRunInfo, ErrorKind, Event, Invocation, ProfileSource, SandboxKind, TrackedCommand,
};
use state::State;

struct Pending {
    command: TrackedCommand,
    started: Instant,
}

static PENDING: Lazy<Mutex<Option<Pending>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    Block,
}

pub struct AgentRunStart {
    pub profile_source: ProfileSource,
    pub remote: bool,
    pub sandbox_backend: SandboxKind,
    pub worktree: bool,
    /// Policy counts exist only for local runs with the audit log on: the
    /// counters are fed by the audit log, and remote runs enforce policy
    /// server-side. Otherwise they are omitted rather than reported as zero.
    pub counts_available: bool,
}

static AGENT_RUN: Lazy<Mutex<Option<AgentRunStart>>> = Lazy::new(|| Mutex::new(None));
static AGENT_LAUNCHED: AtomicBool = AtomicBool::new(false);
static POLICY_ALLOW: AtomicU32 = AtomicU32::new(0);
static POLICY_DENY: AtomicU32 = AtomicU32::new(0);
static POLICY_BLOCK: AtomicU32 = AtomicU32::new(0);

/// Records the funnel values of an `agent run` once its profile is resolved.
/// They are only reported if `mark_agent_run_launched` is also called, so a
/// run that fails while the audit log, proxy or sandbox are being set up does
/// not count as launched. Never pass paths, names or policy contents.
pub fn prepare_agent_run(
    profile_source: ProfileSource,
    remote: bool,
    sandbox_backend: SandboxKind,
    worktree: bool,
    audit_log_enabled: bool,
) {
    *lock(&AGENT_RUN) = Some(AgentRunStart {
        profile_source,
        remote,
        sandbox_backend,
        worktree,
        counts_available: !remote && audit_log_enabled,
    });
}

/// Called at the point the agent process is about to start, after the proxy
/// and sandbox are up. A no-op for commands that never prepared an agent run.
pub fn mark_agent_run_launched() {
    AGENT_LAUNCHED.store(true, Ordering::SeqCst);
}

/// Classifies a proxy audit action by kind only. The action string is never
/// stored or sent. Sandbox filesystem denials are not HTTP policy decisions
/// and are deliberately not counted.
pub fn classify_action(action: &str) -> Option<PolicyDecision> {
    match action {
        "filesystem_denied" => None,
        "connect_allowed" | "injected" | "injected_upgrade" | "forwarded" | "mcp_tool_call" => {
            Some(PolicyDecision::Allow)
        }
        "unknown_placeholder" => Some(PolicyDecision::Block),
        other if other.ends_with("_denied") => Some(PolicyDecision::Deny),
        _ => None,
    }
}

/// Counts one policy decision. Cheap and lock-free; safe to call from the
/// proxy's request path.
pub fn count_action(action: &str) {
    let counter = match classify_action(action) {
        Some(PolicyDecision::Allow) => &POLICY_ALLOW,
        Some(PolicyDecision::Deny) => &POLICY_DENY,
        Some(PolicyDecision::Block) => &POLICY_BLOCK,
        None => return,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// The funnel fields, but only for a run that was both prepared and
/// launched. A run that failed in between has none, which is how a failed
/// start differs from a started run in the data.
fn launched_agent_run(
    prepared: Option<&AgentRunStart>,
    launched: bool,
    allow: u32,
    deny: u32,
    block: u32,
) -> Option<AgentRunInfo> {
    prepared
        .filter(|_| launched)
        .map(|start| agent_run_info(start, allow, deny, block))
}

/// Builds the funnel fields; counts are omitted unless they are available.
pub fn agent_run_info(start: &AgentRunStart, allow: u32, deny: u32, block: u32) -> AgentRunInfo {
    let counts = |value: u32| start.counts_available.then_some(value);
    AgentRunInfo {
        profile_source: start.profile_source,
        remote: start.remote,
        sandbox_backend: start.sandbox_backend,
        worktree: start.worktree,
        policy_allow: counts(allow),
        policy_deny: counts(deny),
        policy_block: counts(block),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registers the invocation if (and only if) it is a tracked command. It is
/// given only the parsed command, never the raw arguments.
pub fn begin(entity: &EntityType) {
    if let Some(command) = TrackedCommand::from_entity(entity) {
        *lock(&PENDING) = Some(Pending {
            command,
            started: Instant::now(),
        });
    }
}

/// Shows the first-run notice on `out` and remembers that it was shown.
/// Returns false, writing nothing, if it cannot be remembered, so the user
/// is never nagged on every run.
pub fn show_notice_once(path: &Path, state: &mut State, out: &mut impl Write) -> bool {
    state.notice_shown = true;
    if state::save(path, state).is_err() {
        return false;
    }
    consent::write_notice(out);
    true
}

/// Reports the finished invocation, at most once per process (the first call
/// wins). Never fails and never changes the exit code. Only `Exit::terminate`
/// calls it; `failure` is the category of a failed command, including one that
/// exits 0.
pub fn finish(exit_code: i32, failure: Option<ErrorKind>) {
    let Some(pending) = lock(&PENDING).take() else {
        return;
    };

    let Some(path) = state::state_path() else {
        return;
    };
    let signals = Signals::from_process();
    let mut state = state::load(&path);

    match consent::decide(&signals, &state) {
        Decision::Off => {}
        Decision::ShowNotice => {
            show_notice_once(&path, &mut state, &mut std::io::stderr());
        }
        Decision::DebugPrint => {
            // Display only: nothing is saved and nothing is sent. Without a
            // stored ID, show a throwaway one rather than creating state.
            let install_id = state.install_id.unwrap_or_else(Uuid::new_v4);
            send::print_debug(&make_event(
                pending, failure, exit_code, &signals, install_id,
            ));
        }
        Decision::Send => {
            let (install_id, created) = state.ensure_install_id();
            if created && state::save(&path, &state).is_err() {
                return; // an ID that cannot be remembered would inflate install counts
            }
            let event = make_event(pending, failure, exit_code, &signals, install_id);
            send::dispatch(&event);
        }
    }
}

fn make_event(
    pending: Pending,
    failure: Option<ErrorKind>,
    exit_code: i32,
    signals: &Signals,
    install_id: Uuid,
) -> Event {
    let aborted = crate::REQUEST_ABORTED.load(Ordering::SeqCst);
    let prepared = lock(&AGENT_RUN).take();
    let agent_run = launched_agent_run(
        prepared.as_ref(),
        AGENT_LAUNCHED.load(Ordering::SeqCst),
        POLICY_ALLOW.load(Ordering::Relaxed),
        POLICY_DENY.load(Ordering::Relaxed),
        POLICY_BLOCK.load(Ordering::Relaxed),
    );
    let recorded_error = never_launched_error(
        pending.command,
        agent_run.is_some(),
        exit_code,
        aborted,
        failure,
    );
    Event::new(
        Invocation {
            command: pending.command,
            duration_ms: pending.started.elapsed().as_millis() as u64,
            exit_code,
            aborted,
            recorded_error,
            is_tty: signals.interactive,
            agent_run,
        },
        install_id,
    )
}

/// `handle_cli` states its own failures through `Exit`, but the run handlers
/// it calls (`handle_load_env_run` and what it calls) print many errors, such
/// as missing secrets or a declined prompt, and return `Ok(())`, which
/// `handle_cli` can only see as success. An `agent run` that never reached its
/// launch point (`mark_agent_run_launched`) is a failed start, not a successful
/// run; report it as a validation error so it cannot inflate the funnel.
/// Anything already failed, aborted or exiting non-zero is left as it is.
fn never_launched_error(
    command: TrackedCommand,
    launched: bool,
    exit_code: i32,
    aborted: bool,
    recorded: Option<ErrorKind>,
) -> Option<ErrorKind> {
    if command == TrackedCommand::AgentRun
        && !launched
        && exit_code == 0
        && !aborted
        && recorded.is_none()
    {
        Some(ErrorKind::Validation)
    } else {
        recorded
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::telemetry::state::State;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stashbase-telemetry-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("telemetry.json")
    }

    #[test]
    fn notice_is_written_once_and_remembered() {
        let path = temp_path("notice");
        let mut state = State::default();
        let mut out = Vec::new();
        assert!(show_notice_once(&path, &mut state, &mut out));
        assert!(!out.is_empty());
        assert!(state::load(&path).notice_shown);
    }

    #[test]
    fn notice_is_skipped_when_it_cannot_be_remembered() {
        let dir =
            std::env::temp_dir().join(format!("stashbase-telemetry-blocked-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("file");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("telemetry.json"); // parent is a file: save fails

        let mut state = State::default();
        let mut out = Vec::new();
        assert!(!show_notice_once(&path, &mut state, &mut out));
        assert!(
            out.is_empty(),
            "must stay silent rather than repeat the notice every run"
        );
    }

    #[test]
    fn no_raw_process_exit_outside_the_allowlist() {
        // Every exit reachable from a tracked command must go through
        // `Exit::terminate` so its outcome is reported. Allowed: `terminate`
        // itself, this file (the check below names the string), main.rs (a
        // forced exit on a second Ctrl-C) and the untracked `scans` commands.
        const ALLOWED: &[&str] = &[
            "src/exit.rs",
            "src/telemetry/mod.rs",
            "src/main.rs",
            "src/handlers/scans/",
        ];

        fn visit(dir: &Path, root: &Path, offenders: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, root, offenders);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if ALLOWED.iter().any(|allowed| relative.starts_with(allowed)) {
                    continue;
                }
                if std::fs::read_to_string(&path)
                    .unwrap()
                    .contains("std::process::exit")
                {
                    offenders.push(relative);
                }
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut offenders = Vec::new();
        visit(&root.join("src"), root, &mut offenders);
        assert!(
            offenders.is_empty(),
            "return an Exit (or call Exit::terminate) instead of std::process::exit in: {offenders:?}"
        );
    }

    #[test]
    fn finish_without_begin_is_a_no_op() {
        // No pending command is registered in this test process; this must
        // return immediately without touching the filesystem or network.
        finish(0, None);
        finish(1, Some(ErrorKind::Other));
    }

    #[test]
    fn actions_are_classified_by_kind_only() {
        for (action, expected) in [
            ("connect_allowed", Some(PolicyDecision::Allow)),
            ("injected", Some(PolicyDecision::Allow)),
            ("injected_upgrade", Some(PolicyDecision::Allow)),
            ("forwarded", Some(PolicyDecision::Allow)),
            ("mcp_tool_call", Some(PolicyDecision::Allow)),
            ("host_denied", Some(PolicyDecision::Deny)),
            ("credential_rule_denied", Some(PolicyDecision::Deny)),
            ("mcp_tool_denied", Some(PolicyDecision::Deny)),
            ("unknown_placeholder", Some(PolicyDecision::Block)),
            // Sandbox filesystem denials are not HTTP policy decisions.
            ("filesystem_denied", None),
            ("session_started", None),
            ("session_stopped", None),
            ("session_expired", None),
            ("upgrade_closed", None),
            ("tls_trust_failed", None),
            ("request_invalid", None),
            // The proxy refusing a telemetry POST is not a user policy decision.
            ("telemetry_blocked", None),
            ("upstream_timeout", None),
            ("something_new", None),
        ] {
            assert_eq!(classify_action(action), expected, "{action}");
        }
    }

    #[test]
    fn counts_are_reported_only_for_local_runs_with_the_audit_log_on() {
        let local = AgentRunStart {
            profile_source: event::ProfileSource::Directory,
            remote: false,
            sandbox_backend: event::SandboxKind::Docker,
            worktree: true,
            counts_available: true,
        };
        let info = agent_run_info(&local, 4, 2, 1);
        assert_eq!(
            (info.policy_allow, info.policy_deny, info.policy_block),
            (Some(4), Some(2), Some(1))
        );
        assert!(info.sandbox_backend == event::SandboxKind::Docker && !info.remote);

        for (remote, audit_log) in [(true, true), (true, false), (false, false)] {
            let start = AgentRunStart {
                profile_source: event::ProfileSource::Global,
                remote,
                sandbox_backend: event::SandboxKind::Native,
                worktree: false,
                counts_available: !remote && audit_log,
            };
            let info = agent_run_info(&start, 4, 2, 1);
            assert_eq!(
                (info.policy_allow, info.policy_deny, info.policy_block),
                (None, None, None),
                "remote={remote} audit_log={audit_log}"
            );
        }
    }

    #[test]
    fn counting_increments_the_matching_counter() {
        // Other tests may count concurrently, so only assert on increase.
        let before = POLICY_DENY.load(std::sync::atomic::Ordering::Relaxed);
        count_action("host_denied");
        count_action("session_started"); // not a decision: must not count
        assert!(POLICY_DENY.load(std::sync::atomic::Ordering::Relaxed) > before);
    }

    #[test]
    fn an_agent_run_that_never_launched_is_a_failed_start() {
        // Several pre-launch validation failures print a message and return
        // normally (exit 0); they must not be reported as a successful run.
        assert_eq!(
            never_launched_error(TrackedCommand::AgentRun, false, 0, false, None),
            Some(ErrorKind::Validation)
        );
    }

    #[test]
    fn launched_aborted_failed_or_other_commands_are_left_alone() {
        use TrackedCommand::*;
        // Launched: a clean exit really is a success.
        assert_eq!(never_launched_error(AgentRun, true, 0, false, None), None);
        // Ctrl-C before launch stays "aborted", a non-zero exit stays an error,
        // and an error that was already reported keeps its category.
        assert_eq!(never_launched_error(AgentRun, false, 0, true, None), None);
        assert_eq!(never_launched_error(AgentRun, false, 1, false, None), None);
        assert_eq!(
            never_launched_error(AgentRun, false, 0, false, Some(ErrorKind::Network)),
            Some(ErrorKind::Network)
        );
        // Only `agent run` has a launch point.
        assert_eq!(never_launched_error(Setup, false, 0, false, None), None);
        assert_eq!(never_launched_error(AgentInit, false, 0, false, None), None);
    }

    #[test]
    fn funnel_fields_need_both_preparation_and_launch() {
        let start = || AgentRunStart {
            profile_source: event::ProfileSource::Directory,
            remote: false,
            sandbox_backend: event::SandboxKind::Native,
            worktree: false,
            counts_available: true,
        };
        // Prepared and launched: reported.
        assert!(launched_agent_run(Some(&start()), true, 1, 2, 3).is_some());
        // Prepared but the proxy or child never started: not reported.
        assert!(launched_agent_run(Some(&start()), false, 1, 2, 3).is_none());
        // Never prepared: not reported.
        assert!(launched_agent_run(None, true, 1, 2, 3).is_none());
    }
}
