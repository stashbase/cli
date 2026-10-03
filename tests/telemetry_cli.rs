//! End-to-end tests of telemetry through the real binary.
//!
//! Every test runs with an isolated `HOME`, a scrubbed environment and an API
//! URL nothing listens on, so nothing can reach a real server. Most tests use
//! `STASHBASE_TELEMETRY_DEBUG=1`, which prints the exact event to stderr
//! instead of sending it. The delivery tests start a local server instead.
//!
//! The delivery tests and the `agent run` test need loopback ports and, for
//! `agent run`, the macOS sandbox, so they cannot run inside another sandbox
//! (for example Claude Code's); they skip themselves when the OS refuses.
#![cfg(unix)]

use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use uuid::Uuid;

const BIN: &str = env!("CARGO_BIN_EXE_stashbase");
const DEBUG_PREFIX: &str = "telemetry (debug, not sent): ";

/// Variables that would change telemetry behaviour if the developer running
/// the tests happens to have them set.
const SCRUBBED: &[&str] = &[
    "STASHBASE_API_KEY",
    "STASHBASE_TELEMETRY",
    "DO_NOT_TRACK",
    "STASHBASE_SANDBOX",
    "STASHBASE_TELEMETRY_DEBUG",
    "STASHBASE_TELEMETRY_WORKER",
    "CI",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "BUILDKITE",
    "CIRCLECI",
    "JENKINS_URL",
    "TF_BUILD",
    "XDG_CONFIG_HOME",
];

/// The fields every event carries. `agent run` adds more once it has launched;
/// `error_kind` appears only on errors.
const BASE_KEYS: &[&str] = &[
    "arch",
    "cli_version",
    "command",
    "duration_ms",
    "event",
    "event_id",
    "install_id",
    "is_tty",
    "os",
    "outcome",
    "timestamp_ms",
];

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("stashbase-it-{}", Uuid::new_v4()));
        let home = root.join("home");
        let cwd = root.join("project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        let _ = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Self { root, home, cwd }
    }

    fn command(&self, args: &[&str], api_url: &str, debug: bool) -> Command {
        let mut command = Command::new(BIN);
        command.args(args).current_dir(&self.cwd);
        for name in SCRUBBED {
            command.env_remove(name);
        }
        command
            .env("HOME", &self.home)
            .env("STASHBASE_API_URL", api_url)
            // The tests talk to local servers, which only an explicit telemetry
            // destination allows (a non-default API URL alone turns it off).
            .env("STASHBASE_TELEMETRY_URL", api_url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if debug {
            command.env("STASHBASE_TELEMETRY_DEBUG", "1");
        }
        command
    }

    /// Runs in debug mode against an API URL nothing listens on.
    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, &[])
    }

    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = self.command(args, "http://127.0.0.1:1", true);
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    /// A human has agreed to telemetry (this also counts as having seen the
    /// first-run notice, which cannot be shown without a terminal).
    fn enable(&self) {
        let out = self.run(&["config", "telemetry", "enable"]);
        assert!(out.status.success(), "{}", stderr(&out));
    }

    /// Where the CLI keeps its config file for this isolated HOME.
    fn config_file(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home
                .join("Library/Application Support/stashbase/config.toml")
        } else {
            self.home.join(".config/stashbase/config.toml")
        }
    }

    fn write_profile(&self, name: &str, body: &str) {
        let dir = self.cwd.join(".stashbase/agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.toml")), body).unwrap();
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn events(out: &Output) -> Vec<Value> {
    stderr(out)
        .lines()
        .filter_map(|line| line.strip_prefix(DEBUG_PREFIX))
        .map(|json| serde_json::from_str(json).expect("a debug line must be valid JSON"))
        .collect()
}

fn keys(event: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = event
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn agent_init_reports_exactly_one_event_with_the_expected_shape() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let out = sandbox.run(&["agent", "init", "p"]);

    assert!(out.status.success(), "{}", stderr(&out));
    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    let event = &events[0];
    assert_eq!(keys(event), BASE_KEYS);
    assert_eq!(event["event"], "cli_command");
    assert_eq!(event["command"], "agent init");
    assert_eq!(event["outcome"], "ok");
    assert_eq!(event["is_tty"], false);
    Uuid::parse_str(event["install_id"].as_str().unwrap()).unwrap();
    Uuid::parse_str(event["event_id"].as_str().unwrap()).unwrap();
    assert!(event["timestamp_ms"].as_i64().unwrap() > 1_600_000_000_000);
}

#[test]
fn a_failed_agent_init_reports_an_error_with_a_category() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    sandbox.run(&["agent", "init", "dup"]);

    let out = sandbox.run(&["agent", "init", "dup"]); // refuses to overwrite

    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["outcome"], "error");
    assert!(
        ["auth", "network", "not_found", "validation", "other"]
            .contains(&events[0]["error_kind"].as_str().unwrap()),
        "{}",
        events[0]
    );
}

#[test]
fn pull_and_push_each_report_one_event_when_they_fail() {
    let sandbox = Sandbox::new(); // an empty project: no stashbase.yaml
    sandbox.enable();

    for command in ["pull", "push"] {
        let out = sandbox.run(&[command, "--api-key", "dummy-key"]);

        let events = events(&out);
        assert_eq!(events.len(), 1, "{command}: {}", stderr(&out));
        assert_eq!(events[0]["command"], command);
        assert_eq!(events[0]["outcome"], "error", "{command}: {}", events[0]);
        assert_eq!(keys(&events[0]).len(), BASE_KEYS.len() + 1, "{}", events[0]); // + error_kind
        assert!(!events[0].to_string().contains("dummy-key"));
    }
}

#[test]
fn config_telemetry_manages_the_setting() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let off = sandbox.run(&["config", "telemetry", "disable"]);
    assert!(off.status.success(), "{}", stderr(&off));
    assert!(
        events(&off).is_empty(),
        "config telemetry must never report itself"
    );
    let after_off = sandbox.run(&["agent", "init", "a"]);
    assert!(events(&after_off).is_empty());

    let status = sandbox.run(&["config", "telemetry", "status"]);
    let text = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(text.contains("disabled"), "{text}");
    assert!(events(&status).is_empty());

    let on = sandbox.run(&["config", "telemetry", "enable"]);
    assert!(on.status.success(), "{}", stderr(&on));
    let after_on = sandbox.run(&["agent", "init", "b"]);
    assert_eq!(events(&after_on).len(), 1);
}

/// There is no top-level `telemetry` command: the setting lives under `config`.
#[test]
fn the_top_level_telemetry_command_is_gone() {
    let sandbox = Sandbox::new();

    let out = sandbox.run(&["telemetry", "status"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("unrecognized subcommand"),
        "{}",
        stderr(&out)
    );
}

/// Opting out must never depend on the config file being readable, or a broken
/// config.toml would trap the user in telemetry.
#[test]
fn config_telemetry_works_even_when_config_toml_is_broken() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    std::fs::create_dir_all(sandbox.config_file().parent().unwrap()).unwrap();
    std::fs::write(sandbox.config_file(), "this is = = not toml [[[\n").unwrap();

    let off = sandbox.run(&["config", "telemetry", "disable"]);
    assert!(off.status.success(), "{}", stderr(&off));
    let status = sandbox.run(&["config", "telemetry", "status"]);
    let text = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(text.contains("disabled"), "{text} / {}", stderr(&status));
}

#[test]
fn there_is_no_telemetry_reset_command() {
    let sandbox = Sandbox::new();

    let out = sandbox.run(&["config", "telemetry", "reset"]);

    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn config_print_shows_the_telemetry_state() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let enabled = sandbox.run(&["config", "print"]);
    let text = String::from_utf8_lossy(&enabled.stdout).into_owned();
    assert!(text.contains("# telemetry: enabled"), "{text}");
    assert!(events(&enabled).is_empty());

    sandbox.run(&["config", "telemetry", "disable"]);
    let disabled = sandbox.run(&["config", "print"]);
    let text = String::from_utf8_lossy(&disabled.stdout).into_owned();
    assert!(
        text.contains("# telemetry: disabled (opted out with"),
        "{text}"
    );

    let in_ci = sandbox.run_with(&["config", "print"], &[("CI", "true")]);
    let text = String::from_utf8_lossy(&in_ci.stdout).into_owned();
    assert!(
        text.contains("# telemetry: disabled (running in CI)"),
        "{text}"
    );
}

#[test]
fn run_and_secrets_schema_pull_report_by_their_own_labels() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    for (args, label) in [
        (&["run", "--", "true"][..], "run"),
        (
            &["secrets", "schema", "pull", "-p", "p", "-e", "e"][..],
            "secrets schema pull",
        ),
    ] {
        let out = sandbox.run(args); // no API key: fails fast, offline
        let events = events(&out);
        assert_eq!(events.len(), 1, "{label}: {}", stderr(&out));
        assert_eq!(events[0]["command"], label);
        assert_eq!(events[0]["error_kind"], "auth", "{label}: {}", events[0]);
    }
}

#[test]
fn scan_install_reports_but_the_scans_themselves_do_not() {
    let sandbox = Sandbox::new(); // already a git repository
    sandbox.enable();

    let install = sandbox.run(&["scan", "install", "pre-commit"]);
    let events_install = events(&install);
    assert_eq!(events_install.len(), 1, "{}", stderr(&install));
    assert_eq!(events_install[0]["command"], "scan install");
    assert_eq!(events_install[0]["outcome"], "ok", "{}", stderr(&install));

    // The checks run from git hooks on every commit and must never report.
    for args in [
        &["scan", "staged"][..],
        &["scan", "changes"][..],
        &["scan", "unpushed"][..],
    ] {
        let out = sandbox.run(args);
        assert!(
            events(&out).is_empty(),
            "{args:?} reported: {}",
            stderr(&out)
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn agent_run_reports_whether_it_used_a_worktree() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    sandbox.write_profile("p", "egress_hosts = [\"example.com\"]\n");
    let committed = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(&sandbox.cwd)
        .status()
        .unwrap();
    assert!(committed.success());

    let with = sandbox.run(&[
        "agent",
        "run",
        "--profile",
        "p",
        "--worktree",
        "--",
        "sh",
        "-c",
        "exit 0",
    ]);
    if stderr(&with).contains("Operation not permitted") {
        eprintln!("skipping: the macOS sandbox cannot be applied from inside another sandbox");
        return;
    }
    assert_eq!(events(&with).len(), 1, "{}", stderr(&with));
    assert_eq!(events(&with)[0]["worktree"], true, "{}", stderr(&with));

    let without = sandbox.run(&["agent", "run", "--profile", "p", "--", "sh", "-c", "exit 0"]);
    assert_eq!(
        events(&without)[0]["worktree"],
        false,
        "{}",
        stderr(&without)
    );
}

#[test]
fn other_secrets_commands_report_nothing() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    for args in [
        &["secrets", "list", "-p", "p", "-e", "e"][..],
        &["secrets", "get", "-p", "p", "-e", "e", "NAME"][..],
    ] {
        let out = sandbox.run(args);
        assert!(
            events(&out).is_empty(),
            "{args:?} reported: {}",
            stderr(&out)
        );
    }
}

#[test]
fn a_missing_api_key_is_reported_as_an_auth_error() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let out = sandbox.run(&["pull"]); // no --api-key, no STASHBASE_API_KEY

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["outcome"], "error");
    assert_eq!(events[0]["error_kind"], "auth", "{}", events[0]);
}

/// `handle_cli` prints these two failures and returns normally (exit 0), so
/// telemetry must record them itself or it would report a success.
#[test]
fn a_malformed_config_file_is_reported_as_an_error_not_a_success() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    std::fs::create_dir_all(sandbox.config_file().parent().unwrap()).unwrap();
    std::fs::write(sandbox.config_file(), "this is = = not toml [[[\n").unwrap();

    let out = sandbox.run(&["pull", "--api-key", "dummy-key"]);

    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["command"], "pull");
    assert_eq!(events[0]["outcome"], "error", "{}", events[0]);
    assert_eq!(events[0]["error_kind"], "validation", "{}", events[0]);
}

#[test]
fn an_unknown_profile_is_reported_as_an_error_not_a_success() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let out = sandbox.run_with(
        &["pull", "--api-key", "dummy-key"],
        &[("STASHBASE_PROFILE", "SENTINEL-no-such-profile")],
    );

    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["outcome"], "error", "{}", events[0]);
    assert_eq!(events[0]["error_kind"], "validation", "{}", events[0]);
    assert!(!events[0].to_string().contains("SENTINEL"), "{}", events[0]);
}

#[test]
fn a_missing_config_file_is_reported_as_a_validation_error() {
    let sandbox = Sandbox::new(); // no stashbase.yaml
    sandbox.enable();

    let out = sandbox.run(&["pull", "--api-key", "dummy-key"]);

    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["error_kind"], "validation", "{}", events[0]);
}

#[test]
fn the_event_never_contains_what_the_user_typed() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let out = sandbox.run(&["agent", "init", "SENTINEL_PROFILE_NAME"]);

    let all = stderr(&out);
    let event_lines: Vec<&str> = all
        .lines()
        .filter(|l| l.starts_with(DEBUG_PREFIX))
        .collect();
    assert_eq!(event_lines.len(), 1);
    assert!(!event_lines[0].contains("SENTINEL"), "{}", event_lines[0]);
    assert!(
        !event_lines[0].contains(sandbox.cwd.to_str().unwrap()),
        "{}",
        event_lines[0]
    );
}

#[test]
fn commands_outside_the_funnel_report_nothing() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    for args in [
        &["doctor"][..],
        &["generate", "uuid"][..],
        &["config", "telemetry", "status"][..],
        &["agent", "profiles", "list"][..],
        &["agent", "logs", "list"][..],
    ] {
        let out = sandbox.run(args);
        assert!(
            events(&out).is_empty(),
            "{args:?} reported: {}",
            stderr(&out)
        );
    }
}

#[test]
fn every_off_switch_suppresses_the_event() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    for (index, (key, value)) in [
        ("STASHBASE_TELEMETRY", "0"),
        ("DO_NOT_TRACK", "1"),
        ("CI", "true"),
        ("STASHBASE_SANDBOX", "1"),
        // Any value of the marker counts: an agent cannot switch the
        // suppression off by setting it to "0" or "false".
        ("STASHBASE_SANDBOX", "0"),
        ("STASHBASE_SANDBOX", "false"),
        // The marker was scrubbed, but the agent proxy's CA variables remain
        // (a local session, then a remote one).
        ("SSL_CERT_FILE", "/tmp/stashbase-proxy-ca-0c9d1f2e.pem"),
        (
            "NODE_EXTRA_CA_CERTS",
            "/home/u/.cache/stashbase/remote-proxy-abc123.pem",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("off{index}");
        let out = sandbox.run_with(&["agent", "init", &name], &[(key, value)]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(events(&out).is_empty(), "{key}={value} still reported");
    }
}

#[test]
fn an_ordinary_ca_bundle_does_not_suppress_the_event() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    // A developer's own CA setup must not be mistaken for an agent session.
    let out = sandbox.run_with(
        &["agent", "init", "a"],
        &[("SSL_CERT_FILE", "/etc/ssl/cert.pem")],
    );
    assert_eq!(events(&out).len(), 1, "{}", stderr(&out));
}

#[test]
fn a_non_default_api_url_sends_nothing_unless_the_destination_is_explicit() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    let run = |api_url: &str, telemetry_url: Option<&str>| {
        let mut command = sandbox.command(
            &["agent", "init", &Uuid::new_v4().to_string()],
            api_url,
            true,
        );
        command.env_remove("STASHBASE_TELEMETRY_URL");
        if let Some(url) = telemetry_url {
            command.env("STASHBASE_TELEMETRY_URL", url);
        }
        events(&command.output().unwrap()).len()
    };

    // Self-hosted or staging server: nothing is sent to it.
    assert_eq!(run("https://stashbase.example.com", None), 0);
    assert_eq!(run("http://127.0.0.1:1", None), 0);
    // Stashbase's own service, however it is spelled, is fine.
    assert_eq!(run("https://api.stashbase.dev", None), 1);
    assert_eq!(run("https://API.stashbase.dev/", None), 1);
    // An explicit telemetry URL is the opt-in to send somewhere else.
    assert_eq!(
        run("https://stashbase.example.com", Some("http://127.0.0.1:1")),
        1
    );
}

#[test]
fn disable_and_enable_switch_telemetry() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    sandbox.run(&["config", "telemetry", "disable"]);
    let off = sandbox.run(&["agent", "init", "a"]);
    assert!(events(&off).is_empty());

    sandbox.run(&["config", "telemetry", "enable"]);
    let on = sandbox.run(&["agent", "init", "b"]);
    assert_eq!(events(&on).len(), 1);
}

#[test]
fn debug_mode_works_before_the_notice_and_saves_no_install_id() {
    let sandbox = Sandbox::new(); // telemetry never enabled, notice never shown

    let first = events(&sandbox.run(&["agent", "init", "a"]));
    let second = events(&sandbox.run(&["agent", "init", "b"]));

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    // The ID is a throwaway: debug mode must not create any telemetry state.
    assert_ne!(first[0]["install_id"], second[0]["install_id"]);
}

#[test]
fn an_agent_run_that_fails_to_start_is_not_reported_as_a_success() {
    let sandbox = Sandbox::new();
    sandbox.enable();

    let out = sandbox.run(&["agent", "run", "--profile", "does-not-exist", "--", "true"]);

    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["command"], "agent run");
    // The CLI prints "profile not found" and exits 0; telemetry must still
    // call it a failed start, and report no funnel fields.
    assert_eq!(events[0]["outcome"], "error", "{}", events[0]);
    assert_eq!(events[0]["error_kind"], "validation");
    for key in [
        "profile_source",
        "remote",
        "sandbox_backend",
        "worktree",
        "policy_allow",
    ] {
        assert!(events[0].get(key).is_none(), "{key} present");
    }
}

#[test]
fn an_agent_run_that_fails_after_validation_but_before_launch_reports_no_funnel_fields() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    sandbox.write_profile("p", "egress_hosts = [\"example.com\"]\n");

    // The profile resolves, then --remote rejects it (no secret bindings):
    // after validation, but before anything is launched.
    let out = sandbox.run(&["agent", "run", "--profile", "p", "--remote", "--", "true"]);

    let events = events(&out);
    assert_eq!(events.len(), 1, "{}", stderr(&out));
    assert_eq!(events[0]["command"], "agent run");
    assert_eq!(events[0]["outcome"], "error", "{}", events[0]);
    for key in [
        "profile_source",
        "remote",
        "sandbox_backend",
        "worktree",
        "policy_allow",
    ] {
        assert!(
            events[0].get(key).is_none(),
            "{key} reported for a run that never launched: {}",
            events[0]
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn agent_run_passes_the_exit_code_through_and_reports_the_funnel_fields() {
    let sandbox = Sandbox::new();
    sandbox.enable();
    sandbox.write_profile("p", "egress_hosts = [\"example.com\"]\n");

    let failing = sandbox.run(&["agent", "run", "--profile", "p", "--", "sh", "-c", "exit 3"]);
    if stderr(&failing).contains("Operation not permitted") {
        eprintln!("skipping: the macOS sandbox cannot be applied from inside another sandbox");
        return;
    }

    assert_eq!(failing.status.code(), Some(3), "{}", stderr(&failing));
    let events_failing = events(&failing);
    assert_eq!(events_failing.len(), 1, "{}", stderr(&failing));
    let event = &events_failing[0];
    assert_eq!(event["command"], "agent run");
    assert_eq!(event["outcome"], "error");
    assert_eq!(event["profile_source"], "directory");
    assert_eq!(event["remote"], false);
    assert_eq!(event["sandbox_backend"], "native");
    assert_eq!(event["worktree"], false);
    for key in ["policy_allow", "policy_deny", "policy_block"] {
        assert_eq!(event[key], 0, "{key}");
    }

    let passing = sandbox.run(&["agent", "run", "--profile", "p", "--", "sh", "-c", "exit 0"]);
    assert_eq!(passing.status.code(), Some(0), "{}", stderr(&passing));
    assert_eq!(events(&passing)[0]["outcome"], "ok");
}

/// Accepts one connection, reads the whole request and answers 204. Returns
/// the request text through the channel.
fn capture_one_request(listener: TcpListener) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buf = vec![0u8; 16 * 1024];
        let mut total = 0;
        loop {
            let Ok(n) = stream.read(&mut buf[total..]) else {
                return;
            };
            if n == 0 {
                break;
            }
            total += n;
            let text = String::from_utf8_lossy(&buf[..total]).to_ascii_lowercase();
            if let Some(head_end) = text.find("\r\n\r\n") {
                let length = text
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if total >= head_end + 4 + length {
                    break;
                }
            }
        }
        let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n");
        let _ = sender.send(String::from_utf8_lossy(&buf[..total]).into_owned());
    });
    receiver
}

/// Accepts connections for the rest of the process and keeps every request
/// body it receives, answering each with 204.
fn capture_all_requests(listener: TcpListener) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = bodies.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let sink = sink.clone();
            thread::spawn(move || {
                let mut stream = stream;
                let mut buf = vec![0u8; 16 * 1024];
                let mut total = 0;
                loop {
                    let Ok(n) = stream.read(&mut buf[total..]) else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    total += n;
                    let text = String::from_utf8_lossy(&buf[..total]).to_ascii_lowercase();
                    if let Some(head_end) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if total >= head_end + 4 + length {
                            break;
                        }
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n");
                let text = String::from_utf8_lossy(&buf[..total]).into_owned();
                if let Some(body) = text.split("\r\n\r\n").nth(1) {
                    sink.lock().unwrap().push(body.to_owned());
                }
            });
        }
    });
    bodies
}

fn audit_actions(home: &std::path::Path) -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
                    if let Ok(value) = serde_json::from_str::<Value>(line) {
                        if let Some(action) = value["action"].as_str() {
                            out.push(action.to_owned());
                        }
                    }
                }
            }
        }
    }
    let mut actions = Vec::new();
    walk(home, &mut actions);
    actions
}

fn bind_loopback() -> Option<(TcpListener, String)> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let url = format!("http://{}", listener.local_addr().ok()?);
    Some((listener, url))
}

#[test]
fn the_detached_sender_delivers_the_event_to_the_endpoint() {
    let Some((listener, url)) = bind_loopback() else {
        eprintln!("skipping: loopback ports are not available");
        return;
    };
    let received = capture_one_request(listener);
    let sandbox = Sandbox::new();
    sandbox.enable();

    // Not in debug mode: this really sends, through the detached sender.
    let out = sandbox
        .command(&["agent", "init", "p"], &url, false)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));

    let request = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the detached sender never delivered the event");
    assert!(request.starts_with("POST /v1/telemetry"), "{request}");
    assert!(request
        .to_ascii_lowercase()
        .contains("content-type: application/json"));
    let body = request.split("\r\n\r\n").nth(1).unwrap();
    let event: Value = serde_json::from_str(body).unwrap();
    assert_eq!(event["command"], "agent init");
    assert_eq!(keys(&event), BASE_KEYS);
}

/// What a tracked CLI run inside a real native `agent run` session reports,
/// after `scrub` (an `env -u ...` prefix) has removed some of the session's
/// environment. The profile allows the "API" host, so only Stashbase's own
/// suppression can stop the event. Returns the commands the server received
/// and the proxy's audit actions, or `None` if the OS refused the sandbox.
#[cfg(target_os = "macos")]
fn reports_from_inside_a_session(scrub: &str) -> Option<(Vec<String>, Vec<String>)> {
    let Some((listener, url)) = bind_loopback() else {
        eprintln!("skipping: loopback ports are not available");
        return None;
    };
    let received = capture_all_requests(listener);
    let sandbox = Sandbox::new();
    sandbox.enable();
    sandbox.write_profile("p", "egress_hosts = [\"127.0.0.1\"]\n");

    // Run a tracked command inside the session, and keep the session open
    // long enough for its sender to try.
    let inside = format!("{scrub} {BIN} pull >/dev/null 2>&1; sleep 2");
    let out = sandbox
        .command(
            &["agent", "run", "--profile", "p", "--", "sh", "-c", &inside],
            &url,
            false,
        )
        .output()
        .unwrap();
    if stderr(&out).contains("Operation not permitted") {
        eprintln!("skipping: the macOS sandbox cannot be applied from inside another sandbox");
        return None;
    }
    assert!(out.status.success(), "{}", stderr(&out));

    let commands_now = || -> Vec<String> {
        received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|body| serde_json::from_str::<Value>(body).ok())
            .filter_map(|event| event["command"].as_str().map(str::to_owned))
            .collect()
    };
    // The host's own `agent run` event does arrive; wait for it.
    let deadline = Instant::now() + Duration::from_secs(8);
    while !commands_now().iter().any(|c| c == "agent run") && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(500)); // let any stray event land
    Some((commands_now(), audit_actions(&sandbox.home)))
}

/// A harness that strips `STASHBASE_*` variables but keeps the proxy and CA
/// variables: the CLI recognises the agent proxy's CA files and stays silent
/// on its own, so the proxy never even sees a telemetry request.
#[cfg(target_os = "macos")]
#[test]
fn a_scrubbed_marker_alone_does_not_get_telemetry_out() {
    let Some((commands, actions)) = reports_from_inside_a_session("env -u STASHBASE_SANDBOX")
    else {
        return;
    };
    assert_eq!(
        commands,
        vec!["agent run".to_owned()],
        "the inner pull must not report"
    );
    assert!(
        !actions.iter().any(|a| a == "telemetry_blocked"),
        "the CLI should not have tried at all: {actions:?}"
    );
}

/// An agent that removes the marker and the CA variables on purpose still
/// cannot get an event out: every request of a local session passes through
/// the agent proxy, which refuses the telemetry POST whatever the profile's
/// egress policy allows.
#[cfg(target_os = "macos")]
#[test]
fn removing_the_marker_and_the_ca_variables_still_hits_the_proxy_block() {
    let scrub = "env -u STASHBASE_SANDBOX -u SSL_CERT_FILE -u CURL_CA_BUNDLE -u GIT_SSL_CAINFO \
                 -u NODE_EXTRA_CA_CERTS -u CODEX_CA_CERTIFICATE";
    let Some((commands, actions)) = reports_from_inside_a_session(scrub) else {
        return;
    };
    assert_eq!(
        commands,
        vec!["agent run".to_owned()],
        "the inner pull must not report"
    );
    // Positive evidence that the inner CLI really tried, and the proxy stopped it.
    assert!(
        actions.iter().any(|a| a == "telemetry_blocked"),
        "the proxy never logged the blocked telemetry POST: {actions:?}"
    );
}

fn fastest_of(runs: usize, mut run: impl FnMut() -> Output) -> Duration {
    (0..runs)
        .map(|_| {
            let started = Instant::now();
            let out = run();
            let elapsed = started.elapsed();
            assert!(out.status.success(), "{}", stderr(&out));
            elapsed
        })
        .min()
        .unwrap()
}

#[test]
fn a_hanging_endpoint_does_not_delay_the_command() {
    let Some((listener, url)) = bind_loopback() else {
        eprintln!("skipping: loopback ports are not available");
        return;
    };
    // Accepts connections and never answers.
    thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            held.push(stream);
        }
    });
    let sandbox = Sandbox::new();
    sandbox.enable();
    let counter = std::cell::Cell::new(0);
    let name = || {
        counter.set(counter.get() + 1);
        format!("p{}", counter.get())
    };

    let baseline = fastest_of(3, || {
        sandbox
            .command(&["agent", "init", &name()], &url, false)
            .env("STASHBASE_TELEMETRY", "0")
            .output()
            .unwrap()
    });
    let with_telemetry = fastest_of(3, || {
        sandbox
            .command(&["agent", "init", &name()], &url, false)
            .output()
            .unwrap()
    });

    // Waiting on the hung request would add its whole timeout (500 ms or more).
    assert!(
        with_telemetry < baseline + Duration::from_millis(300),
        "telemetry added {:?} (baseline {:?}, with telemetry {:?})",
        with_telemetry.saturating_sub(baseline),
        baseline,
        with_telemetry
    );
}
