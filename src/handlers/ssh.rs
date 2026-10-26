//! `stashbase ssh install <host>`: installs the CLI on a machine through the
//! user's own `ssh`, so their ~/.ssh/config, agent and jump hosts apply, and
//! optionally logs it in. Stashbase never wraps the SSH session itself.

use std::{
    io::{BufRead, BufReader, IsTerminal, Write},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};

use crate::{
    cmd::ssh::{SshCommand, SshInstall, SshSubcommand},
    handlers::run::entry::shell_quote,
    utils::{interaction::input_password, output::ColorizeIfColoredOutput},
};

const INSTALL_SCRIPT: &str = include_str!("../../install.sh");

pub fn handle_ssh_command(cmd: SshCommand) -> Result<()> {
    match cmd.subcommand {
        SshSubcommand::Install(args) => install(args),
    }
}

fn install(cmd: SshInstall) -> Result<()> {
    let host = cmd.host;
    validate_host(&host)?;
    if which_ssh().is_none() {
        bail!("`ssh` was not found on this machine.");
    }

    eprintln!("Installing stashbase on {host} over SSH…");
    let binary = install_over_ssh(&host)?;

    // Only a key typed or pasted for this machine is sent: the key this CLI
    // uses is never copied onto another machine.
    let api_key = if !cmd.no_login && std::io::stdin().is_terminal() {
        input_password(&format!("API key for {host} (leave empty to skip)"))
    } else {
        None
    };
    match api_key {
        Some(key) => set_remote_api_key(&host, &binary, &key)?,
        None => eprintln!(
            "Not logged in. To log in later: ssh {host}, then `stashbase config api-key set`."
        ),
    }

    check_login_path(&host, &binary);
    let version = remote_output(&host, &format!("{} --version", shell_quote(&binary)))?;
    eprintln!(
        "\n{} {} on {host}. Connect with `ssh {host}` and use `stashbase` there.",
        "Done:".green_if_tty_stderr(),
        version.trim()
    );
    Ok(())
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
fn install_over_ssh(host: &str) -> Result<String> {
    let mut child = Command::new("ssh")
        .args([host, "bash -s"])
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
        bail!("Installing on {host} failed (ssh exited with {status}).");
    }
    binary.context("install.sh finished without saying where it installed stashbase")
}

/// Warns when `stashbase` in the user's login shell on the host isn't the
/// binary just installed. Best effort: a failed check only skips the warning.
fn check_login_path(host: &str, binary: &str) {
    let Ok(output) = Command::new("ssh")
        .args([host, r#""${SHELL:-sh}" -lc 'command -v stashbase'"#])
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

/// The key goes through ssh's stdin, never the command line, so it shows up
/// in no process list or shell history on either machine.
fn set_remote_api_key(host: &str, binary: &str, api_key: &str) -> Result<()> {
    let mut child = Command::new("ssh")
        .args([
            host,
            &format!("{} config api-key set --stdin", shell_quote(binary)),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Could not start `ssh`")?;
    child
        .stdin
        .take()
        .context("ssh stdin")?
        .write_all(api_key.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        bail!("Setting the API key on {host} failed (ssh exited with {status}).");
    }
    Ok(())
}

fn remote_output(host: &str, command: &str) -> Result<String> {
    let output = Command::new("ssh")
        .args([host, command])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .context("Could not start `ssh`")?;
    if !output.status.success() {
        bail!(
            "`{command}` failed on {host} (ssh exited with {}).",
            output.status
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
    fn rejects_hosts_ssh_would_read_as_options() {
        assert!(validate_host("mybox").is_ok());
        assert!(validate_host("dev@10.0.0.5").is_ok());
        assert!(validate_host("-oProxyCommand=sh").is_err());
        assert!(validate_host("my box").is_err());
        assert!(validate_host("").is_err());
    }
}
