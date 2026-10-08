//! Characterization tests for how the CLI exits: the process exit code, the
//! message a user sees, and (for tracked commands) the outcome telemetry
//! reports. They run the real binary with an isolated `HOME`, a scrubbed
//! environment and an API URL nothing listens on.
//!
//! A command that reports an error exits non-zero, so scripts and CI see the
//! failure. The one exception is the bare `agent hooks` invocation, which
//! coding agents run before tool calls and whose exit code they read: its
//! failures still exit 0. Changing an exit code is a product decision, so a
//! test here must not change as a side effect of refactoring how the CLI exits.
//!
//! The tests that apply the macOS sandbox or bind a loopback port skip
//! themselves when the OS refuses (for example inside another sandbox).
#![cfg(unix)]

use std::{
    io::Read,
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use uuid::Uuid;

const BIN: &str = env!("CARGO_BIN_EXE_stashbase");
const DEBUG_PREFIX: &str = "telemetry (debug, not sent): ";
const DEAD_API: &str = "http://127.0.0.1:1";

/// Variables that would change behaviour if the developer running the tests
/// happens to have them set.
const SCRUBBED: &[&str] = &[
    "STASHBASE_API_KEY",
    "STASHBASE_PROFILE",
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
    "FORCE_COLOR",
    "CLICOLOR_FORCE",
    "NO_COLOR",
    "NOCOLOR",
];

/// What telemetry is expected to have reported for the invocation.
enum Reported {
    /// Exactly one event with this outcome and, for errors, this category.
    Event(&'static str, Option<&'static str>),
    /// No event: the command is not tracked.
    Nothing,
    /// A known gap: the command is tracked but today reports a failure as a
    /// success. Not asserted, so fixing it does not break the test.
    NotAsserted,
}

struct Project {
    root: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
}

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("stashbase-exit-{}", Uuid::new_v4()));
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
        let project = Self { root, home, cwd };
        // Consent, so debug mode prints the event it would have sent.
        let out = project.run(&["config", "telemetry", "enable"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        project
    }

    fn command(&self, args: &[&str], api_url: &str) -> Command {
        let mut command = Command::new(BIN);
        command.args(args).current_dir(&self.cwd);
        for name in SCRUBBED {
            command.env_remove(name);
        }
        command
            .env("HOME", &self.home)
            .env("STASHBASE_API_URL", api_url)
            .env("STASHBASE_TELEMETRY_URL", api_url)
            .env("STASHBASE_TELEMETRY_DEBUG", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args, DEAD_API).output().unwrap()
    }

    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = self.command(args, DEAD_API);
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn config_file(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home
                .join("Library/Application Support/stashbase/config.toml")
        } else {
            self.home.join(".config/stashbase/config.toml")
        }
    }

    fn break_config(&self) {
        std::fs::create_dir_all(self.config_file().parent().unwrap()).unwrap();
        std::fs::write(self.config_file(), "this is = = not toml [[[\n").unwrap();
    }

    fn write_profile(&self, name: &str, body: &str) {
        let dir = self.cwd.join(".stashbase/agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.toml")), body).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn events(out: &Output) -> Vec<Value> {
    text(&out.stderr)
        .lines()
        .filter_map(|line| line.strip_prefix(DEBUG_PREFIX))
        .map(|json| serde_json::from_str(json).expect("a debug line must be valid JSON"))
        .collect()
}

/// Asserts the exit code, that `needle` appears in the combined output, and
/// what telemetry reported.
fn check(label: &str, out: &Output, code: i32, needle: &str, reported: Reported) {
    let all = format!("{}{}", text(&out.stdout), text(&out.stderr));
    assert_eq!(out.status.code(), Some(code), "{label} exit code:\n{all}");
    assert!(
        all.contains(needle),
        "{label}: missing {needle:?} in:\n{all}"
    );
    let events = events(out);
    match reported {
        Reported::Event(outcome, kind) => {
            assert_eq!(events.len(), 1, "{label}: expected one event:\n{all}");
            assert_eq!(events[0]["outcome"], outcome, "{label}: {}", events[0]);
            assert_eq!(
                events[0].get("error_kind").and_then(Value::as_str),
                kind,
                "{label}: {}",
                events[0]
            );
        }
        Reported::Nothing => assert!(events.is_empty(), "{label}: unexpected event:\n{all}"),
        Reported::NotAsserted => {}
    }
}

// ---- config and profile resolution --------------------------------------

#[test]
fn a_malformed_config_toml_fails_every_command_that_reads_it() {
    let project = Project::new();
    project.break_config();

    let message = "Could not parse config file";
    // Untracked commands: nothing is reported.
    for args in [&["config", "print"][..], &["config", "profile", "list"]] {
        check(
            &args.join(" "),
            &project.run(args),
            1,
            message,
            Reported::Nothing,
        );
    }
    // Tracked commands report the failure.
    let failed = Reported::Event("error", Some("validation"));
    check(
        "pull",
        &project.run(&["pull", "--api-key", "k"]),
        1,
        message,
        failed,
    );
    check(
        "setup",
        &project.run(&["setup"]),
        1,
        message,
        Reported::Event("error", Some("validation")),
    );
    check(
        "agent init",
        &project.run(&["agent", "init", "p"]),
        1,
        message,
        Reported::Event("error", Some("validation")),
    );
}

#[test]
fn config_reset_still_works_when_the_config_is_malformed() {
    let project = Project::new();
    project.break_config();

    let out = project.run(&["config", "reset", "--force"]);
    let all = format!("{}{}", text(&out.stdout), text(&out.stderr));
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(events(&out).is_empty(), "{all}");
    assert!(!all.contains("Could not parse config file"), "{all}");
}

#[test]
fn an_unknown_profile_environment_variable_fails_with_exit_1() {
    let project = Project::new();

    let out = project.run_with(
        &["pull", "--api-key", "k"],
        &[("STASHBASE_PROFILE", "no-such-profile")],
    );

    check(
        "unknown profile",
        &out,
        1,
        "no-such-profile",
        Reported::Event("error", Some("validation")),
    );
}

#[test]
fn a_config_command_on_an_unknown_profile_prints_the_error_and_exits_1() {
    let project = Project::new();

    let out = project.run(&["config", "profile", "use", "nosuch"]);

    check("config", &out, 1, "was not found", Reported::Nothing);
}

#[test]
fn a_config_setter_that_fails_exits_1() {
    let project = Project::new();

    let out = project.run(&["config", "profile", "add", "default"]);

    check(
        "config profile add default",
        &out,
        1,
        "'default' is the implicit profile",
        Reported::Nothing,
    );
}

// ---- API key and project file --------------------------------------------

#[test]
fn a_missing_api_key_exits_1_for_pull_push_and_run() {
    let project = Project::new();
    let auth = || Reported::Event("error", Some("auth"));

    check(
        "pull",
        &project.run(&["pull"]),
        1,
        "API key is required",
        auth(),
    );
    check(
        "push",
        &project.run(&["push"]),
        1,
        "API key is required",
        auth(),
    );
    check(
        "run",
        &project.run(&["run", "--", "true"]),
        1,
        "API key is required",
        auth(),
    );
    // An untracked command exits the same way and reports nothing.
    check(
        "secrets list",
        &project.run(&["secrets", "list"]),
        1,
        "API key is required",
        Reported::Nothing,
    );
}

#[test]
fn a_missing_api_key_in_json_mode_prints_a_json_error_and_exits_1() {
    let project = Project::new();

    let out = project.run(&["pull", "--json"]);

    check(
        "pull --json",
        &out,
        1,
        "\"type\": \"authentication_error\"",
        Reported::Event("error", Some("auth")),
    );
}

#[test]
fn a_missing_stashbase_yaml_prints_the_error_and_exits_1() {
    let project = Project::new();
    let failed = || Reported::Event("error", Some("validation"));

    for command in ["pull", "push"] {
        check(
            command,
            &project.run(&[command, "--api-key", "k"]),
            1,
            "No 'stashbase.yaml' file found",
            failed(),
        );
    }
}

#[test]
fn conflicting_scope_flags_print_the_error_and_exit_1() {
    let project = Project::new();

    for (args, message) in [
        (
            &["pull", "--scope", "environment", "--api-key", "k"][..],
            "Target file is required when using environment scope",
        ),
        (
            &["push", "--scope", "environment", "--api-key", "k"][..],
            "Target file is required when using environment scope",
        ),
        (
            &[
                "run",
                "--scope",
                "environment",
                "--project",
                "p",
                "--api-key",
                "k",
                "--",
                "true",
            ][..],
            "Cannot use --scope=environment with --project",
        ),
    ] {
        check(
            args[0],
            &project.run(args),
            1,
            message,
            Reported::NotAsserted,
        );
    }
}

#[test]
fn an_api_request_that_fails_exits_1() {
    let project = Project::new();

    let out = project.run(&[
        "secrets",
        "list",
        "-p",
        "myproj",
        "-e",
        "dev",
        "--api-key",
        "k",
    ]);

    check(
        "secrets list",
        &out,
        1,
        "Could not connect to the API",
        Reported::Nothing,
    );
}

/// Without `--only`, `run` fetches every secret of the environment. Against
/// an API nothing listens on, that fetch fails and the command never starts.
#[test]
fn run_without_only_fetches_secrets_before_starting_the_command() {
    let project = Project::new();

    let out = project.run(&[
        "run",
        "-p",
        "myproj",
        "-e",
        "dev",
        "--api-key",
        "k",
        "--",
        "sh",
        "-c",
        "echo child-ran",
    ]);

    check(
        "run",
        &out,
        1,
        "Could not connect to the API",
        Reported::NotAsserted,
    );
    assert!(!text(&out.stdout).contains("child-ran"));
}

/// A `--json` error replaces the loading spinner instead of being appended to
/// its line, so the error is the only thing left to parse.
#[test]
fn run_json_error_clears_the_loading_spinner() {
    let project = Project::new();
    std::fs::write(project.cwd.join("secrets.env"), "OTHER=1\n").unwrap();

    let out = project.run(&[
        "run",
        "--file",
        "secrets.env",
        "--only",
        "MISSING",
        "--api-key",
        "k",
        "--json",
        "--",
        "true",
    ]);

    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("no secrets found"), "{stderr}");
    assert!(!stderr.contains("Loading environment...{"), "{stderr}");
}

#[test]
fn run_without_any_secrets_does_not_start_the_command_and_exits_1() {
    let project = Project::new();
    std::fs::write(project.cwd.join("empty.env"), "").unwrap();

    let out = project.run(&[
        "run",
        "--file",
        "empty.env",
        "--api-key",
        "k",
        "--",
        "sh",
        "-c",
        "echo child-ran",
    ]);

    check(
        "run",
        &out,
        1,
        "No secrets found",
        Reported::Event("error", Some("not_found")),
    );
    assert!(!text(&out.stdout).contains("child-ran"));
}

// ---- setup, doctor, generate ---------------------------------------------

#[test]
fn setup_without_a_terminal_prints_the_error_and_exits_1() {
    let project = Project::new();

    let out = project.run(&["setup"]);

    check(
        "setup",
        &out,
        1,
        "not a terminal",
        Reported::Event("error", Some("other")),
    );
}

#[test]
fn doctor_exits_1_when_a_check_fails_and_0_otherwise() {
    let project = Project::new();

    let healthy = project.run(&["doctor"]);
    check(
        "doctor",
        &healthy,
        0,
        "Stashbase CLI Doctor",
        Reported::Nothing,
    );

    project.break_config();
    let broken = project.run(&["doctor"]);
    check(
        "doctor",
        &broken,
        1,
        "[FAIL] Config file",
        Reported::Nothing,
    );
}

#[test]
fn generate_prints_its_error_and_exits_1() {
    let project = Project::new();

    // ssh-keygen is not on PATH, so key generation fails inside the handler.
    let out = project.run_with(&["generate", "ssh-keypair"], &[("PATH", "/nonexistent")]);

    check(
        "generate",
        &out,
        1,
        "Failed to execute ssh-keygen",
        Reported::Nothing,
    );
}

// ---- agent init -----------------------------------------------------------

#[test]
fn agent_init_refuses_to_overwrite_and_exits_1() {
    let project = Project::new();
    check(
        "first",
        &project.run(&["agent", "init", "dup"]),
        0,
        "Created agent profile",
        Reported::Event("ok", None),
    );

    let out = project.run(&["agent", "init", "dup"]);

    check(
        "second",
        &out,
        1,
        "Refusing to overwrite",
        Reported::Event("error", Some("other")),
    );
}

/// Coding agents run the bare `agent hooks` before tool calls and read its
/// exit code, so its failures keep exiting 0.
#[test]
fn the_bare_agent_hook_still_exits_0_when_it_fails() {
    use std::io::Write;

    let project = Project::new();
    let mut child = project
        .command(&["agent", "hooks", "--api-key", "k"], DEAD_API)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    let out = child.wait_with_output().unwrap();

    check(
        "agent hooks",
        &out,
        0,
        "Agent hook input was not valid JSON",
        Reported::NotAsserted,
    );
}

// ---- agent run: failures before launch -------------------------------------

#[test]
fn agent_run_failures_before_launch_print_the_error_and_exit_1() {
    let project = Project::new();
    project.write_profile("p", "egress_hosts = [\"example.com\"]\n");
    project.write_profile("broken", "workspace = 1\n");
    project.write_profile("bad-source", "[secrets.A]\n");

    for (label, args, message, kind) in [
        (
            "profile not found",
            &["agent", "run", "--profile", "nope", "--", "true"][..],
            "was not found in the global or directory config",
            "validation",
        ),
        (
            "policy file not found",
            &[
                "agent",
                "run",
                "--profile",
                "p",
                "--policy-file",
                "/nonexistent.toml",
                "--",
                "true",
            ][..],
            "Could not resolve agent policy file",
            "other",
        ),
        (
            "unparseable profile",
            &["agent", "run", "--profile", "broken", "--", "true"][..],
            "Could not parse agent profile file",
            "other",
        ),
        (
            "invalid profile",
            &["agent", "run", "--profile", "bad-source", "--", "true"][..],
            "Agent profile is invalid",
            "other",
        ),
        (
            "remote without secret bindings",
            &["agent", "run", "--profile", "p", "--remote", "--", "true"][..],
            "--remote requires Stashbase-managed secret",
            "other",
        ),
        (
            "resume without a worktree",
            &[
                "agent",
                "run",
                "--profile",
                "p",
                "--resume",
                "x",
                "--worktree=false",
                "--",
                "true",
            ][..],
            "can't be combined with --worktree=false",
            "other",
        ),
    ] {
        check(
            label,
            &project.run(args),
            1,
            message,
            Reported::Event("error", Some(kind)),
        );
    }
}

#[test]
fn agent_run_with_personal_credentials_but_no_remote_exits_1() {
    let project = Project::new();
    project.write_profile(
        "personal",
        "[personal_credentials.X]\nhosts = [\"api.example.com\"]\n",
    );

    let out = project.run(&["agent", "run", "--profile", "personal", "--", "true"]);

    check(
        "personal credentials",
        &out,
        1,
        "Personal credential bindings require Remote Agent execution",
        Reported::Event("error", Some("other")),
    );
}

#[cfg(target_os = "macos")]
#[test]
fn agent_run_passes_the_childs_exit_status_through() {
    let project = Project::new();
    project.write_profile("p", "egress_hosts = [\"example.com\"]\n");

    for (script, code, outcome) in [
        ("exit 0", 0, "ok"),
        ("exit 3", 3, "error"),
        // Killed by a signal: no exit code, which is reported as 1.
        ("kill -9 $$", 1, "error"),
    ] {
        let out = project.run(&["agent", "run", "--profile", "p", "--", "sh", "-c", script]);
        if text(&out.stderr).contains("Operation not permitted") {
            eprintln!("skipping: the macOS sandbox cannot be applied from inside another sandbox");
            return;
        }
        assert_eq!(
            out.status.code(),
            Some(code),
            "{script}: {}",
            text(&out.stderr)
        );
        let events = events(&out);
        assert_eq!(events.len(), 1, "{script}: {}", text(&out.stderr));
        assert_eq!(events[0]["outcome"], outcome, "{script}: {}", events[0]);
    }
}

// ---- commands whose Ok(true) means "a check failed" -----------------------

#[test]
fn validation_commands_exit_1_when_a_check_fails() {
    let project = Project::new();

    let out = project.run(&["agent", "validate", "--profile", "nope"]);

    check(
        "agent validate",
        &out,
        1,
        "Profile validation failed",
        Reported::Nothing,
    );
}

#[test]
fn agent_policy_test_on_a_missing_profile_prints_the_error_and_exits_1() {
    let project = Project::new();

    let out = project.run(&["agent", "policy", "test", "--profile", "nope"]);

    check(
        "agent policy test",
        &out,
        1,
        "was not found in the global or directory config",
        Reported::Nothing,
    );
}

// ---- Ctrl-C ---------------------------------------------------------------

/// A request that is aborted with Ctrl-C ends with "Request aborted", exit
/// 130, and an `aborted` outcome (not an error).
#[test]
fn ctrl_c_during_a_request_exits_130_and_reports_aborted() {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        eprintln!("skipping: cannot bind a loopback port here");
        return;
    };
    let url = format!("http://{}", listener.local_addr().unwrap());
    // Accept and never answer, so the request hangs until it is aborted.
    thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
            let _ = stream.read(&mut [0u8; 1024]);
            held.push(stream);
        }
    });
    let project = Project::new();
    let out_file = project.cwd.join("out.env");

    let mut child = project
        .command(
            &[
                "pull",
                "--scope",
                "environment",
                "--file",
                out_file.to_str().unwrap(),
                "--api-key",
                "k",
            ],
            &url,
        )
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(800));
    let _ = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status();
    let started = Instant::now();
    let out = loop {
        if child.try_wait().unwrap().is_some() {
            break child.wait_with_output().unwrap();
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("the command did not stop after Ctrl-C");
        }
        thread::sleep(Duration::from_millis(50));
    };

    check(
        "ctrl-c",
        &out,
        130,
        "Request aborted",
        Reported::Event("aborted", None),
    );
}

/// The same failures as `conflicting_scope_flags_print_the_error_and_exit_1`,
/// seen from telemetry: they are reported as validation failures.
#[test]
fn conflicting_scope_flags_are_reported_as_validation_failures() {
    let project = Project::new();

    for args in [
        &["pull", "--scope", "environment", "--api-key", "k"][..],
        &["push", "--scope", "environment", "--api-key", "k"][..],
        &[
            "run",
            "--scope",
            "environment",
            "--project",
            "p",
            "--api-key",
            "k",
            "--",
            "true",
        ][..],
    ] {
        let out = project.run(args);
        let all = text(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{}: {all}", args[0]);
        let events = events(&out);
        assert_eq!(events.len(), 1, "{}: {all}", args[0]);
        assert_eq!(events[0]["outcome"], "error", "{}: {}", args[0], events[0]);
        assert_eq!(events[0]["error_kind"], "validation", "{}", args[0]);
    }
}
