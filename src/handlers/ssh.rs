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
    cmd::ssh::{SshCommand, SshInstall, SshSetup, SshSubcommand, SshUninstall},
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
            let status = Command::new("ssh")
                .args(connection.control_options("yes"))
                .args(["-o", "ControlPersist=120", "-f", "-N", host])
                .stdout(Stdio::null())
                .status()
                .context("Could not start `ssh`")?;
            if !status.success() {
                bail!("Could not connect to {host} (ssh exited with {status}).");
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
        let mut command = Command::new("ssh");
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
    let host = match &cmd.subcommand {
        SshSubcommand::Install(args) => &args.host,
        SshSubcommand::Setup(args) => &args.host,
        SshSubcommand::Uninstall(args) => &args.host,
    };
    validate_host(host)?;
    if which_ssh().is_none() {
        bail!("`ssh` was not found on this machine.");
    }
    match cmd.subcommand {
        SshSubcommand::Install(args) => install(args),
        SshSubcommand::Setup(args) => setup(args),
        SshSubcommand::Uninstall(args) => uninstall(args),
    }
}

fn install(cmd: SshInstall) -> Result<()> {
    let host = cmd.host;
    let ssh = Connection::open(&host)?;

    let platform = ssh.output("uname -sm")?;
    if let Some(reason) = unsupported_platform(&platform) {
        bail!("{host} runs {}, {reason}", platform.trim());
    }

    eprintln!("Installing stashbase on {host} over SSH…");
    let binary = install_over_ssh(&ssh)?;

    check_login_path(&ssh, &binary);
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

/// Runs the machine's own interactive `stashbase setup` in a terminal session.
fn setup(cmd: SshSetup) -> Result<()> {
    let host = cmd.host;
    if !std::io::stdin().is_terminal() {
        bail!("`stashbase ssh setup` is interactive and needs a terminal.");
    }
    let script =
        format!("{LOCATE_BINARY}[ -n \"$bin\" ] || exit {NOT_INSTALLED}\nexec \"$bin\" setup\n");
    let status = Command::new("ssh")
        .args(["-t", &host, &sh_command(&script, &[])])
        .status()
        .context("Could not start `ssh`")?;
    match status.code() {
        Some(0) => Ok(()),
        Some(NOT_INSTALLED) => {
            bail!("stashbase is not installed on {host}. Install it with: stashbase ssh install {host}")
        }
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
fn install_over_ssh(ssh: &Connection) -> Result<String> {
    let mut child = ssh
        .command(false, "bash -s")
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
    fn rejects_hosts_ssh_would_read_as_options() {
        assert!(validate_host("mybox").is_ok());
        assert!(validate_host("dev@10.0.0.5").is_ok());
        assert!(validate_host("-oProxyCommand=sh").is_err());
        assert!(validate_host("my box").is_err());
        assert!(validate_host("").is_err());
    }
}
