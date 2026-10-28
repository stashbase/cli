//! `stashbase ssh install|setup|uninstall <host>`: manages the CLI on a
//! machine through the user's own `ssh`, so their ~/.ssh/config, agent and
//! jump hosts apply. Stashbase never wraps the SSH session itself.

use std::{
    io::{BufRead, BufReader, IsTerminal, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};

use crate::{
    cmd::ssh::{
        Multiplexer, SshCommand, SshDoctor, SshInstall, SshOpen, SshSetup, SshSubcommand,
        SshUninstall,
    },
    handlers::run::entry::shell_quote,
    utils::{interaction::confirm_opt, output::ColorizeIfColoredOutput},
};

const INSTALL_SCRIPT: &str = include_str!("../../install.sh");

/// POSIX sh that sets `bin` to the stashbase the user's login shell runs, or
/// the install.sh default, or empty when neither exists.
const LOCATE_BINARY: &str = r#"bin=$("${SHELL:-sh}" -lc 'command -v stashbase' 2>/dev/null | tail -n 1)
if [ -z "$bin" ] && [ -x "$HOME/.local/bin/stashbase" ]; then bin="$HOME/.local/bin/stashbase"; fi
"#;

/// POSIX sh that sets `dir` to the CLI's config directory (the `directories`
/// crate's choice for each OS).
const LOCATE_CONFIG_DIR: &str = r#"if [ "$(uname -s)" = Darwin ]; then dir="$HOME/Library/Application Support/stashbase"; else dir="${XDG_CONFIG_HOME:-$HOME/.config}/stashbase"; fi
"#;

/// Exit code of the remote scripts when the CLI isn't installed there.
const NOT_INSTALLED: i32 = 3;

/// The user's `ssh`, giving up on a machine that doesn't answer (a stale
/// address, a stopped VM) instead of hanging with nothing on screen.
fn ssh() -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "ConnectTimeout=15"]);
    command
}

/// The error for `ssh` itself failing (exit 255), which only the user's own
/// `ssh` can explain.
fn connect_error(host: &str, status: std::process::ExitStatus) -> anyhow::Error {
    anyhow::anyhow!(
        "Could not connect to {host} (ssh exited with {status}). Check that `ssh {host}` works on its own; `ssh -v {host}` shows why it doesn't."
    )
}

/// One SSH connection shared by every step of a command, so a password,
/// security-key touch or 2FA prompt comes once rather than per step. The
/// first `ssh` opens it in the background (`-f -N`); later ones reuse its
/// control socket; dropping this closes it.
struct Connection {
    host: String,
    /// Directory holding the control socket. `None` where OpenSSH can't
    /// share connections (Windows), so each step connects on its own.
    control_dir: Option<PathBuf>,
}

impl Connection {
    fn open(host: &str) -> Result<Self> {
        // Not a spinner: ssh may ask for a password or to trust the host.
        eprintln!("Connecting to {host}…");
        let mut connection = Self {
            host: host.to_owned(),
            control_dir: None,
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            // Short: a Unix socket path is limited to about 104 bytes.
            let dir = std::env::temp_dir().join(format!(
                "sb-ssh-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..12]
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .with_context(|| format!("Could not create {}", dir.display()))?;
            connection.control_dir = Some(dir);
            let status = ssh()
                .args(connection.control_options("yes"))
                .args(["-o", "ControlPersist=120", "-f", "-N", host])
                .stdout(Stdio::null())
                .status()
                .context("Could not start `ssh`")?;
            if !status.success() {
                return Err(connect_error(host, status));
            }
        }
        Ok(connection)
    }

    fn control_options(&self, master: &str) -> Vec<String> {
        match &self.control_dir {
            Some(dir) => vec![
                "-o".to_owned(),
                format!("ControlPath={}", dir.join("s").display()),
                "-o".to_owned(),
                format!("ControlMaster={master}"),
            ],
            None => Vec::new(),
        }
    }

    /// `ssh [-t] <host> <remote_command>` over the shared connection.
    fn command(&self, tty: bool, remote_command: &str) -> Command {
        let mut command = ssh();
        command.args(self.control_options("no"));
        if tty {
            command.arg("-t");
        }
        command.args([&self.host, remote_command]);
        command
    }

    fn output(&self, remote_command: &str) -> Result<String> {
        let output = self
            .command(false, remote_command)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .context("Could not start `ssh`")?;
        if !output.status.success() {
            bail!(
                "`{remote_command}` failed on {} (ssh exited with {}).",
                self.host,
                output.status
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let Some(dir) = self.control_dir.take() else {
            return;
        };
        let _ = Command::new("ssh")
            .args([
                "-o",
                &format!("ControlPath={}", dir.join("s").display()),
                "-O",
                "exit",
                &self.host,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub fn handle_ssh_command(cmd: SshCommand) -> Result<()> {
    if which_ssh().is_none() {
        bail!("`ssh` was not found on this machine.");
    }
    match cmd.subcommand {
        SshSubcommand::Install(args) => {
            validate_host(&args.host)?;
            install(args)
        }
        SshSubcommand::Setup(args) => {
            validate_host(&args.host)?;
            setup(args)
        }
        SshSubcommand::Doctor(args) => {
            validate_host(&args.host)?;
            doctor(args)
        }
        SshSubcommand::Uninstall(args) => {
            validate_host(&args.host)?;
            uninstall(args)
        }
        SshSubcommand::Open(args) => {
            validate_host(&args.host)?;
            open(args)
        }
    }
}

/// Checks `--session`: tmux turns `.` and `:` into `_` and screen and
/// zellij have their own rules, so only names all three keep as typed.
fn validate_session(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        bail!("'{name}' is not a valid session name: use letters, digits, `-` and `_`.");
    }
    Ok(())
}

/// Remote command that attaches to `session` of `multiplexer`, creating it
/// if needed, so every connection with the same name lands in the same
/// place; without the multiplexer it says so and opens a login shell.
fn open_script(multiplexer: Multiplexer, session: &str) -> Option<String> {
    let (binary, attach) = match multiplexer {
        Multiplexer::Tmux => ("tmux", format!("tmux new-session -A -s {session}")),
        Multiplexer::Screen => ("screen", format!("screen -xRR -S {session}")),
        Multiplexer::Zellij => ("zellij", format!("zellij attach --create {session}")),
        Multiplexer::None => return None,
    };
    // A terminal the machine has no terminfo for (Ghostty's `xterm-ghostty`,
    // kitty's `xterm-kitty`, ...) makes tmux and screen refuse to start.
    Some(format!(
        "if command -v infocmp >/dev/null 2>&1 && ! infocmp \"$TERM\" >/dev/null 2>&1; then export TERM=xterm-256color; fi\n\
         if command -v {binary} >/dev/null 2>&1; then exec {attach}; fi\n\
         echo '{binary} is not installed here, so this is a plain shell: agents stop if the connection drops. Install {binary} to keep them running.' >&2\n\
         exec \"${{SHELL:-sh}}\" -l\n"
    ))
}

/// Connects with the user's own `ssh` and attaches to the session (named
/// `stashbase` by default); detaching or a dropped connection leaves it
/// running.
fn open(cmd: SshOpen) -> Result<()> {
    let host = cmd.host;
    validate_session(&cmd.session)?;
    let forwards = cmd
        .ports
        .iter()
        .map(|spec| port_forward(spec))
        .collect::<Result<Vec<_>>>()?;
    eprintln!("Connecting to {host}…");
    let mut command = ssh();
    for (local, remote) in &forwards {
        eprintln!("Forwarding localhost:{local} to port {remote} on {host}");
        command.args(["-L", &format!("127.0.0.1:{local}:localhost:{remote}")]);
    }
    // Keepalives stop NAT and cloud firewalls from cutting an idle session,
    // such as one where an agent works quietly for a long time.
    command.args(["-o", "ServerAliveInterval=30", "-t", &host]);
    if let Some(script) = open_script(cmd.multiplexer, &cmd.session) {
        command.arg(sh_command(&script, &[]));
    }
    let status = command.status().context("Could not start `ssh`")?;
    // 255 is ssh's own failure; anything else is how the remote shell ended.
    if status.code() == Some(255) {
        return Err(connect_error(&host, status));
    }
    Ok(())
}

/// Parses `--port`: `3000` forwards local 3000 to the machine's 3000,
/// `8080:3000` local 8080 to its 3000. Returns (local, remote).
fn port_forward(spec: &str) -> Result<(u16, u16)> {
    let parse = |port: &str| {
        port.parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .with_context(|| {
                format!("Invalid --port '{spec}': use a port such as 3000, or LOCAL:REMOTE such as 8080:3000.")
            })
    };
    match spec.split_once(':') {
        Some((local, remote)) => Ok((parse(local)?, parse(remote)?)),
        None => {
            let port = parse(spec)?;
            Ok((port, port))
        }
    }
}

fn install(cmd: SshInstall) -> Result<()> {
    let host = cmd.host;
    let ssh = Connection::open(&host)?;

    // One round trip for the platform and any existing copies: the one the
    // login shell runs (`bin`) and others install.sh or PATH might pick.
    let probe = format!(
        "uname -sm\n{LOCATE_BINARY}[ -n \"$bin\" ] && echo \"bin=$bin\" && echo \"version=$(\"$bin\" --version 2>/dev/null)\"\n\
         for f in \"$HOME/.local/bin/stashbase\" /usr/local/bin/stashbase \"$(command -v stashbase 2>/dev/null)\"; do\n\
         [ -n \"$f\" ] && [ -x \"$f\" ] && [ \"$f\" != \"$bin\" ] && echo \"other=$f\"\n\
         done\ntrue\n"
    );
    let found = Probe::parse(&ssh.output(&sh_command(&probe, &[]))?);
    if let Some(reason) = unsupported_platform(&found.platform) {
        bail!("{host} runs {}, {reason}", found.platform);
    }

    let question = install_question(&host, &found);
    if !cmd.yes {
        if !std::io::stdin().is_terminal() {
            bail!("{question} Pass --yes to proceed without confirmation.");
        }
        if confirm_opt(&question) != Some(true) {
            return Ok(());
        }
    }

    eprintln!("Installing stashbase on {host} over SSH…");
    // Update the copy the login shell runs, where the question said: left to
    // itself, install.sh would pick whatever its non-login PATH finds first.
    let install_dir = found
        .bin
        .as_deref()
        .and_then(|bin| bin.rsplit_once('/'))
        .map(|(dir, _)| dir);
    let binary = install_over_ssh(&ssh, install_dir)?;

    check_login_path(&ssh, &binary);
    for other in found.others.iter().filter(|other| **other != binary) {
        eprintln!(
            "{} {host} also has {other}, which `ssh {host}` doesn't run. Remove it if you don't need it.",
            "Note:".yellow_if_tty_stderr()
        );
    }
    let version = ssh.output(&format!("{} --version", shell_quote(&binary)))?;
    // The API key is set by `ssh setup`, like `setup` does locally; the key
    // this CLI uses is never copied onto another machine.
    eprintln!(
        "\n{} {} on {host}. Set it up with: stashbase ssh setup {host}",
        "Done:".green_if_tty_stderr(),
        version.trim()
    );
    Ok(())
}

/// What `ssh install` found on the machine before installing.
#[derive(Debug, Default, PartialEq, Eq)]
struct Probe {
    /// `uname -sm`.
    platform: String,
    /// The stashbase the login shell runs.
    bin: Option<String>,
    version: Option<String>,
    /// Other copies, which the login shell doesn't run.
    others: Vec<String>,
}

impl Probe {
    /// The first line is `uname -sm`; then `bin=`, `version=` and `other=`
    /// lines.
    fn parse(output: &str) -> Self {
        let mut probe = Self {
            platform: output.lines().next().unwrap_or_default().trim().to_owned(),
            ..Self::default()
        };
        for line in output.lines().skip(1) {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match name {
                "bin" => probe.bin = Some(value.to_owned()),
                "version" => probe.version = Some(value.to_owned()),
                "other" if !probe.others.iter().any(|other| other == value) => {
                    probe.others.push(value.to_owned())
                }
                _ => {}
            }
        }
        probe
    }
}

/// What `ssh install` asks before installing or updating.
fn install_question(host: &str, probe: &Probe) -> String {
    let platform = &probe.platform;
    match (probe.bin.as_deref(), probe.version.as_deref()) {
        (Some(bin), Some(version)) => {
            format!("Update stashbase on {host} ({version} at {bin}) to the latest release?")
        }
        (Some(bin), None) => format!("Replace {bin} on {host} with the latest stashbase?"),
        _ => format!("Install stashbase on {host} ({platform})?"),
    }
}

/// Checks `uname -sm` against the targets install.sh knows, before anything
/// is downloaded.
fn unsupported_platform(uname: &str) -> Option<&'static str> {
    let mut parts = uname.split_whitespace();
    // Not `?`: output that isn't two words must not count as supported.
    match (parts.next().unwrap_or(""), parts.next().unwrap_or("")) {
        ("Linux", "x86_64") | ("Darwin", "arm64" | "aarch64") => None,
        ("Linux", "aarch64" | "arm64") => {
            Some("which isn't supported yet: stashbase is released for x86_64 Linux only.")
        }
        ("Darwin", _) => Some("which isn't supported: stashbase needs an Apple Silicon Mac."),
        _ => {
            Some("which stashbase isn't released for (x86_64 Linux and Apple Silicon macOS only).")
        }
    }
}

/// Hint for `agent run` started in an SSH session outside a terminal
/// multiplexer, where a dropped connection ends the agent with it.
pub fn detached_agent_hint(
    env: impl Fn(&str) -> Option<String>,
    interactive: bool,
) -> Option<&'static str> {
    let set = |name: &str| env(name).is_some_and(|value| !value.is_empty());
    let over_ssh = set("SSH_CONNECTION") || set("SSH_TTY");
    let multiplexed = ["TMUX", "STY", "ZELLIJ", "HERDR_ENV"]
        .iter()
        .any(|name| set(name));
    (interactive && over_ssh && !multiplexed).then_some(
        "You're connected over SSH, so the agent stops if the connection drops. Run it inside tmux (or screen) to keep it running.",
    )
}

/// POSIX sh, with `bin` set, that runs the machine's `doctor` and `agent
/// docker doctor` and sets `checks` to the exit status of `doctor`. Docker
/// is needed only by some profiles, so its findings never count as failed.
const CHECKS: &str = "\"$bin\" doctor --auth-check\n\
     checks=$?\n\
     echo\n\
     echo 'Docker sandbox (needed only for agent profiles with backend = \"docker\"):'\n\
     \"$bin\" agent docker doctor\n";

/// Remote command for `ssh setup`: the machine's own interactive setup, then
/// its checks, so a key that doesn't work or a missing Docker shows up now,
/// with its fix, rather than on the first run. The checks don't fail the
/// setup.
fn setup_script() -> String {
    format!(
        "{LOCATE_BINARY}[ -n \"$bin\" ] || exit {NOT_INSTALLED}\n\
         \"$bin\" setup || exit $?\n\
         echo\n\
         {CHECKS}exit 0\n"
    )
}

/// Remote command for `ssh doctor`: the checks `ssh setup` ends with,
/// failing when `doctor` does.
fn doctor_script() -> String {
    format!("{LOCATE_BINARY}[ -n \"$bin\" ] || exit {NOT_INSTALLED}\n{CHECKS}exit $checks\n")
}

/// Runs the machine's checks, after comparing its CLI's version with this
/// one: an older copy there may lack fixes or reject newer profile options.
fn doctor(cmd: SshDoctor) -> Result<()> {
    let host = cmd.host;
    let ssh = Connection::open(&host)?;
    let found = ssh.output(&sh_command(
        &format!("{LOCATE_BINARY}[ -n \"$bin\" ] && \"$bin\" --version\ntrue\n"),
        &[],
    ))?;
    let Some(remote) = found.split_whitespace().last() else {
        bail!(
            "stashbase is not installed on {host}. Install it with: stashbase ssh install {host}"
        );
    };
    let local = env!("CARGO_PKG_VERSION");
    eprintln!("stashbase {remote} on {host} (this computer: {local})");
    if version_is_older(remote, local) {
        eprintln!(
            "{} {host} runs an older stashbase. Update it with: stashbase ssh install {host}",
            "Warning:".yellow_if_tty_stderr()
        );
    }
    eprintln!();

    // A terminal there keeps the checks' colors; ssh allocates one only when
    // its own stdin is a terminal.
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let status = ssh
        .command(tty, &sh_command(&doctor_script(), &[]))
        .status()
        .context("Could not start `ssh`")?;
    match status.code() {
        Some(0) => Ok(()),
        Some(NOT_INSTALLED) => {
            bail!("stashbase is not installed on {host}. Install it with: stashbase ssh install {host}")
        }
        Some(255) => Err(connect_error(&host, status)),
        _ => bail!("The checks on {host} found failures; see above for how to fix them."),
    }
}

/// Whether version `a` comes before `b`, comparing dotted numbers such as
/// `0.23.1`. False when either isn't one, so an odd version never warns.
fn version_is_older(a: &str, b: &str) -> bool {
    let parse = |version: &str| {
        version
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
    };
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

/// Runs the machine's own interactive `stashbase setup` in a terminal session.
fn setup(cmd: SshSetup) -> Result<()> {
    let host = cmd.host;
    if !std::io::stdin().is_terminal() {
        bail!("`stashbase ssh setup` is interactive and needs a terminal.");
    }
    eprintln!("Connecting to {host}…");
    let status = ssh()
        .args(["-t", &host, &sh_command(&setup_script(), &[])])
        .status()
        .context("Could not start `ssh`")?;
    match status.code() {
        Some(0) => {
            eprintln!(
                "\n{} stashbase is set up on {host}. Open a session there with: stashbase ssh open {host}\n\
                 Then log in to your agent there once (for the Docker sandbox, in `stashbase agent docker shell`).",
                "Done:".green_if_tty_stderr()
            );
            Ok(())
        }
        Some(NOT_INSTALLED) => {
            bail!("stashbase is not installed on {host}. Install it with: stashbase ssh install {host}")
        }
        Some(255) => Err(connect_error(&host, status)),
        _ => bail!("Setup on {host} failed (ssh exited with {status})."),
    }
}

/// What `uninstall` found on the machine.
#[derive(Debug, Default, PartialEq, Eq)]
struct Installation {
    binary: Option<String>,
    config_dir: Option<String>,
    agent_profiles: usize,
}

fn parse_installation(output: &str) -> Installation {
    let mut installation = Installation::default();
    for line in output.lines() {
        match line.split_once('=') {
            Some(("bin", value)) if !value.is_empty() => {
                installation.binary = Some(value.to_owned())
            }
            Some(("config", value)) if !value.is_empty() => {
                installation.config_dir = Some(value.to_owned())
            }
            Some(("agent_profiles", value)) => {
                installation.agent_profiles = value.trim().parse().unwrap_or(0)
            }
            _ => {}
        }
    }
    installation
}

fn removal_summary(host: &str, installation: &Installation) -> String {
    let mut summary = format!("This removes from {host}:");
    if let Some(binary) = &installation.binary {
        summary.push_str(&format!("\n  {binary}"));
    }
    if let Some(dir) = &installation.config_dir {
        let profiles = match installation.agent_profiles {
            0 => String::new(),
            1 => ", and 1 global agent profile".to_owned(),
            count => format!(", and {count} global agent profiles"),
        };
        summary.push_str(&format!("\n  {dir} (API keys, config{profiles})"));
    }
    summary
}

/// Removes the binary, the stored API keys (also from a keyring, through the
/// machine's own `config reset`) and the config directory, after showing
/// exactly what goes.
fn uninstall(cmd: SshUninstall) -> Result<()> {
    let host = cmd.host;
    let ssh = Connection::open(&host)?;
    let inspect = format!(
        "{LOCATE_BINARY}{LOCATE_CONFIG_DIR}echo \"bin=$bin\"\n[ -d \"$dir\" ] && echo \"config=$dir\"\necho \"agent_profiles=$(ls \"$dir/agents\"/*.toml 2>/dev/null | wc -l)\"\n"
    );
    let installation = parse_installation(&ssh.output(&sh_command(&inspect, &[]))?);
    if installation.binary.is_none() && installation.config_dir.is_none() {
        eprintln!("stashbase is not installed on {host}; nothing to remove.");
        return Ok(());
    }

    eprintln!("{}", removal_summary(&host, &installation));
    if !cmd.yes {
        if !std::io::stdin().is_terminal() {
            bail!("Pass --yes to remove without confirmation.");
        }
        if confirm_opt("Remove?") != Some(true) {
            return Ok(());
        }
    }

    // `-t` so sudo can ask for a password when the binary sits in a
    // directory the user can't write, such as /usr/local/bin.
    let remove = r#"bin="$1"; dir="$2"
if [ -n "$bin" ]; then "$bin" config reset --force >/dev/null 2>&1 || true; fi
if [ -n "$dir" ]; then rm -rf "$dir"; fi
if [ -n "$bin" ]; then
  if [ -w "$(dirname "$bin")" ]; then rm -f "$bin"; else sudo rm -f "$bin"; fi
fi
"#;
    let args = [
        installation.binary.clone().unwrap_or_default(),
        installation.config_dir.clone().unwrap_or_default(),
    ];
    let status = ssh
        .command(true, &sh_command(remove, &args))
        .status()
        .context("Could not start `ssh`")?;
    if !status.success() {
        bail!("Removing stashbase from {host} failed (ssh exited with {status}).");
    }
    eprintln!(
        "{} Removed stashbase from {host}. If an API key was stored there, revoke it in the dashboard too: deleting the file doesn't invalidate a copy.",
        "Done:".green_if_tty_stderr()
    );
    Ok(())
}

/// The remote login shell may not be POSIX (fish), so scripts run under `sh`,
/// with `args` as `$1`, `$2`, …
fn sh_command(script: &str, args: &[String]) -> String {
    let mut command = format!("sh -c {} sh", shell_quote(script));
    for arg in args {
        command.push(' ');
        command.push_str(&shell_quote(arg));
    }
    command
}

/// A host starting with `-` would be read by `ssh` as an option.
fn validate_host(host: &str) -> Result<()> {
    if host.is_empty() || host.starts_with('-') || host.chars().any(char::is_whitespace) {
        bail!("'{host}' is not a valid SSH host.");
    }
    Ok(())
}

fn which_ssh() -> Option<()> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .any(|dir| dir.join("ssh").is_file())
            .then_some(())
    })
}

/// Runs the bundled install.sh on the host and returns where it put the
/// binary. Its output is shown as it comes; the script downloads and
/// checksums the release on the host itself.
fn install_over_ssh(ssh: &Connection, install_dir: Option<&str>) -> Result<String> {
    // `env` so this also works when the login shell isn't POSIX (fish).
    let remote = match install_dir {
        Some(dir) => format!("env INSTALL_DIR={} bash -s", shell_quote(dir)),
        None => "bash -s".to_owned(),
    };
    let mut child = ssh
        .command(false, &remote)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Could not start `ssh`")?;
    // Dropping stdin after the write sends EOF, so the remote bash runs it.
    child
        .stdin
        .take()
        .context("ssh stdin")?
        .write_all(INSTALL_SCRIPT.as_bytes())?;

    // What install.sh prints after `Installed …` are PATH hints for its
    // non-login shell, which are wrong for `ssh <host>` on most distros;
    // `check_login_path` checks the shell the user will actually get.
    let mut binary = None;
    for line in BufReader::new(child.stdout.take().context("ssh stdout")?).lines() {
        let line = line?;
        if binary.is_none() {
            eprintln!("  {line}");
            binary = installed_binary_path(&line);
        }
    }
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "Installing on {} failed (ssh exited with {status}).",
            ssh.host
        );
    }
    binary.context("install.sh finished without saying where it installed stashbase")
}

/// Warns when `stashbase` in the user's login shell on the host isn't the
/// binary just installed. Best effort: a failed check only skips the warning.
fn check_login_path(ssh: &Connection, binary: &str) {
    let Ok(output) = ssh
        .command(false, r#""${SHELL:-sh}" -lc 'command -v stashbase'"#)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    let found = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if let Some(note) = login_path_note(binary, &found) {
        eprintln!("{} {note}", "Note:".yellow_if_tty_stderr());
    }
}

fn login_path_note(binary: &str, found: &str) -> Option<String> {
    let dir = binary.rsplit_once('/').map_or(binary, |(dir, _)| dir);
    if found == binary {
        None
    } else if found.is_empty() {
        Some(format!(
            "`stashbase` isn't on PATH in your login shell there. Add {dir} to PATH in your shell profile on the machine."
        ))
    } else {
        Some(format!(
            "Your login shell there runs {found}, not the {binary} just installed. Remove the old one or put {dir} first on PATH."
        ))
    }
}

/// install.sh ends with `Installed stashbase <version> to <path>`.
fn installed_binary_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("Installed stashbase ")?;
    let (_, path) = rest.split_once(" to ")?;
    Some(path.trim().to_owned()).filter(|path| !path.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_installed_path_from_install_sh() {
        assert_eq!(
            installed_binary_path("Installed stashbase 0.23.0 to /home/dev/.local/bin/stashbase")
                .as_deref(),
            Some("/home/dev/.local/bin/stashbase")
        );
        assert_eq!(
            installed_binary_path("Installing stashbase 0.23.0 for x86_64…"),
            None
        );
        assert_eq!(
            installed_binary_path("Installed stashbase 0.23.0 to "),
            None
        );
    }

    #[test]
    fn the_bundled_script_still_prints_the_installed_path() {
        assert!(INSTALL_SCRIPT.contains(
            r#"log "Installed ${BINARY_NAME} ${version} to ${TARGET_INSTALL_DIR}/${BINARY_NAME}""#
        ));
    }

    #[test]
    fn notes_when_the_login_shell_misses_the_new_binary() {
        let binary = "/home/dev/.local/bin/stashbase";
        assert_eq!(login_path_note(binary, binary), None);
        assert!(login_path_note(binary, "")
            .unwrap()
            .contains("Add /home/dev/.local/bin to PATH"));
        assert!(login_path_note(binary, "/usr/local/bin/stashbase")
            .unwrap()
            .contains("runs /usr/local/bin/stashbase"));
    }

    #[test]
    fn reads_what_uninstall_found() {
        let found = parse_installation(
            "bin=/home/dev/.local/bin/stashbase\nconfig=/home/dev/.config/stashbase\nagent_profiles=       2\n",
        );
        assert_eq!(
            found,
            Installation {
                binary: Some("/home/dev/.local/bin/stashbase".to_owned()),
                config_dir: Some("/home/dev/.config/stashbase".to_owned()),
                agent_profiles: 2,
            }
        );
        assert_eq!(
            parse_installation("bin=\nagent_profiles=0\n"),
            Installation::default()
        );
    }

    #[test]
    fn removal_summary_names_agent_profiles() {
        let mut found = Installation {
            binary: Some("/usr/local/bin/stashbase".to_owned()),
            config_dir: Some("/home/dev/.config/stashbase".to_owned()),
            agent_profiles: 0,
        };
        assert_eq!(
            removal_summary("mybox", &found),
            "This removes from mybox:\n  /usr/local/bin/stashbase\n  /home/dev/.config/stashbase (API keys, config)"
        );
        found.agent_profiles = 1;
        assert!(removal_summary("mybox", &found)
            .ends_with("(API keys, config, and 1 global agent profile)"));
    }

    #[test]
    fn remote_scripts_run_under_sh_with_quoted_args() {
        assert_eq!(
            sh_command("echo \"$1\"", &["/a b/stashbase".to_owned()]),
            r#"sh -c 'echo "$1"' sh '/a b/stashbase'"#
        );
    }

    #[test]
    fn hints_tmux_only_for_interactive_ssh_sessions_outside_a_multiplexer() {
        let hint = |vars: &[(&str, &str)], interactive: bool| {
            let vars: Vec<(String, String)> = vars
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect();
            detached_agent_hint(
                |name| {
                    vars.iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.clone())
                },
                interactive,
            )
        };
        let ssh = ("SSH_CONNECTION", "10.0.0.2 51234 10.0.0.5 22");
        assert!(hint(&[ssh], true).is_some());
        assert!(hint(&[("SSH_TTY", "/dev/pts/0")], true).is_some());
        assert!(hint(&[ssh], false).is_none());
        assert!(hint(&[], true).is_none());
        for multiplexer in ["TMUX", "STY", "ZELLIJ", "HERDR_ENV"] {
            assert!(
                hint(&[ssh, (multiplexer, "1")], true).is_none(),
                "{multiplexer}"
            );
        }
    }

    #[test]
    fn accepts_only_the_platforms_install_sh_supports() {
        assert_eq!(unsupported_platform("Linux x86_64\n"), None);
        assert_eq!(unsupported_platform("Darwin arm64\n"), None);
        assert!(unsupported_platform("Linux aarch64\n")
            .unwrap()
            .contains("isn't supported yet"));
        assert!(unsupported_platform("Darwin x86_64\n")
            .unwrap()
            .contains("Apple Silicon"));
        assert!(unsupported_platform("FreeBSD amd64\n").is_some());
        assert!(unsupported_platform("").is_some());
        // Kept in step with install.sh's detect_target.
        assert!(INSTALL_SCRIPT.contains("    linux-x86_64)\n"));
        assert!(INSTALL_SCRIPT.contains("    darwin-arm64|darwin-aarch64)\n"));
    }

    #[test]
    fn setup_runs_doctor_only_after_a_successful_setup() {
        let script = setup_script();
        let setup = script.find("\"$bin\" setup || exit $?").unwrap();
        let doctor = script.find("\"$bin\" doctor --auth-check").unwrap();
        let docker = script.find("\"$bin\" agent docker doctor").unwrap();
        assert!(setup < doctor && doctor < docker);
        assert!(script.ends_with("exit 0\n"));
    }

    #[test]
    fn doctor_fails_only_when_the_machines_doctor_does() {
        let script = doctor_script();
        let doctor = script
            .find("\"$bin\" doctor --auth-check\nchecks=$?")
            .unwrap();
        let docker = script.find("\"$bin\" agent docker doctor").unwrap();
        assert!(doctor < docker);
        assert!(script.ends_with("exit $checks\n"));
    }

    #[test]
    fn version_is_older_compares_numbers_not_text() {
        assert!(version_is_older("0.23.0", "0.24.0"));
        assert!(version_is_older("0.9.0", "0.10.0"));
        assert!(!version_is_older("0.24.0", "0.24.0"));
        assert!(!version_is_older("0.25.0", "0.24.0"));
        assert!(!version_is_older("0.24.0-beta", "0.25.0"));
    }

    #[test]
    fn port_forward_takes_one_port_or_local_and_remote() {
        assert_eq!(port_forward("3000").unwrap(), (3000, 3000));
        assert_eq!(port_forward("8080:3000").unwrap(), (8080, 3000));
        for bad in ["", "0", "70000", "abc", "8080:", ":3000", "1:2:3"] {
            assert!(port_forward(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn open_falls_back_to_a_known_terminal_before_attaching() {
        let tmux = open_script(Multiplexer::Tmux, "stashbase").unwrap();
        let fallback = tmux
            .find("! infocmp \"$TERM\" >/dev/null 2>&1; then export TERM=xterm-256color; fi")
            .unwrap();
        assert!(fallback < tmux.find("exec tmux").unwrap());
    }

    #[test]
    fn open_attaches_or_creates_the_named_session() {
        let tmux = open_script(Multiplexer::Tmux, "stashbase").unwrap();
        assert!(tmux.contains("command -v tmux"));
        assert!(tmux.contains("exec tmux new-session -A -s stashbase"));
        assert!(tmux.contains("exec \"${SHELL:-sh}\" -l"));
        assert!(open_script(Multiplexer::Tmux, "work")
            .unwrap()
            .contains("exec tmux new-session -A -s work"));
        assert!(open_script(Multiplexer::Screen, "work")
            .unwrap()
            .contains("exec screen -xRR -S work"));
        assert!(open_script(Multiplexer::Zellij, "work")
            .unwrap()
            .contains("exec zellij attach --create work"));
        assert_eq!(open_script(Multiplexer::None, "work"), None);
    }

    #[test]
    fn session_names_are_kept_to_what_every_multiplexer_accepts() {
        for name in ["stashbase", "work", "api-fix_2"] {
            assert!(validate_session(name).is_ok(), "{name} should be accepted");
        }
        for name in [
            "",
            "-x",
            "a.b",
            "a:b",
            "a b",
            "a;rm",
            "a'b",
            &"x".repeat(65),
        ] {
            assert!(validate_session(name).is_err(), "{name} should be rejected");
        }
    }

    #[test]
    fn install_asks_to_install_or_update() {
        let probe = |output: &str| Probe::parse(output);
        assert_eq!(
            install_question("mybox", &probe("Linux x86_64\n")),
            "Install stashbase on mybox (Linux x86_64)?"
        );
        assert_eq!(
            install_question(
                "mybox",
                &probe("Linux x86_64\nbin=/usr/local/bin/stashbase\nversion=stashbase 0.22.0\n")
            ),
            "Update stashbase on mybox (stashbase 0.22.0 at /usr/local/bin/stashbase) to the latest release?"
        );
        assert_eq!(
            install_question(
                "mybox",
                &probe("Linux x86_64\nbin=/opt/sb/stashbase\nversion=\n")
            ),
            "Replace /opt/sb/stashbase on mybox with the latest stashbase?"
        );
    }

    #[test]
    fn probe_lists_other_copies_once() {
        let probe = Probe::parse(
            "Linux x86_64\nbin=/home/dev/.local/bin/stashbase\nversion=stashbase 0.20.0\nother=/usr/local/bin/stashbase\nother=/usr/local/bin/stashbase\n",
        );
        assert_eq!(
            probe,
            Probe {
                platform: "Linux x86_64".to_owned(),
                bin: Some("/home/dev/.local/bin/stashbase".to_owned()),
                version: Some("stashbase 0.20.0".to_owned()),
                others: vec!["/usr/local/bin/stashbase".to_owned()],
            }
        );
    }

    #[test]
    fn rejects_hosts_ssh_would_read_as_options() {
        assert!(validate_host("mybox").is_ok());
        assert!(validate_host("dev@10.0.0.5").is_ok());
        assert!(validate_host("-oProxyCommand=sh").is_err());
        assert!(validate_host("my box").is_err());
        assert!(validate_host("").is_err());
    }
}
