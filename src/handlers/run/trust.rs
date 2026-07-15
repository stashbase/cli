//! Temporary OS trust-store integration for the broker's per-session CA.
//!
//! This is deliberately opt-in: changing the host trust store is observable state.

use std::{path::Path, process::Command};

#[cfg(target_os = "linux")]
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

pub struct TemporaryCaTrust {
    cleanup: Cleanup,
}

enum Cleanup {
    #[cfg(target_os = "macos")]
    Macos { keychain: String, subject: String },
    #[cfg(target_os = "windows")]
    Windows { subject: String },
    #[cfg(target_os = "linux")]
    Linux {
        path: PathBuf,
        update_command: &'static str,
    },
}

impl Drop for TemporaryCaTrust {
    fn drop(&mut self) {
        match &self.cleanup {
            #[cfg(target_os = "macos")]
            Cleanup::Macos { keychain, subject } => {
                let _ = Command::new("security")
                    .args(["delete-certificate", "-c", subject, "-t", keychain])
                    .status();
            }
            #[cfg(target_os = "windows")]
            Cleanup::Windows { subject } => {
                let _ = Command::new("certutil")
                    .args(["-user", "-delstore", "Root", subject])
                    .status();
            }
            #[cfg(target_os = "linux")]
            Cleanup::Linux {
                path,
                update_command,
            } => {
                let _ = Command::new("sudo").args(["rm", "-f"]).arg(path).status();
                let _ = Command::new("sudo").arg(update_command).status();
            }
        }
    }
}

pub fn install(certificate: &Path, subject: &str) -> Result<TemporaryCaTrust> {
    install_platform(certificate, subject)
}

#[cfg(target_os = "macos")]
fn install_platform(certificate: &Path, subject: &str) -> Result<TemporaryCaTrust> {
    let output = Command::new("security")
        .args(["default-keychain", "-d", "user"])
        .output()
        .context("failed to locate the login Keychain")?;
    if !output.status.success() {
        bail!("failed to locate the login Keychain");
    }
    let keychain = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('"')
        .to_owned();
    run(
        Command::new("security")
            .args(["add-trusted-cert", "-d", "-r", "trustRoot", "-k", &keychain])
            .arg(certificate),
        "failed to trust the temporary broker CA in the login Keychain",
    )?;
    Ok(TemporaryCaTrust {
        cleanup: Cleanup::Macos {
            keychain,
            subject: subject.to_owned(),
        },
    })
}

#[cfg(target_os = "windows")]
fn install_platform(certificate: &Path, subject: &str) -> Result<TemporaryCaTrust> {
    run(
        Command::new("certutil")
            .args(["-user", "-addstore", "Root"])
            .arg(certificate),
        "failed to trust the temporary broker CA in the current-user Root store",
    )?;
    Ok(TemporaryCaTrust {
        cleanup: Cleanup::Windows {
            subject: subject.to_owned(),
        },
    })
}

#[cfg(target_os = "linux")]
fn install_platform(certificate: &Path, _subject: &str) -> Result<TemporaryCaTrust> {
    let (directory, update_command) = if Path::new("/usr/sbin/update-ca-certificates").exists()
        || Path::new("/usr/bin/update-ca-certificates").exists()
    {
        ("/usr/local/share/ca-certificates", "update-ca-certificates")
    } else if Path::new("/usr/bin/update-ca-trust").exists()
        || Path::new("/usr/sbin/update-ca-trust").exists()
    {
        ("/etc/pki/ca-trust/source/anchors", "update-ca-trust")
    } else {
        bail!("unsupported Linux trust store; install update-ca-certificates or update-ca-trust, or run without --trust-broker-ca")
    };
    let target = Path::new(directory).join(format!("stashbase-broker-{}.crt", std::process::id()));
    run(
        Command::new("sudo")
            .args(["install", "-m", "0644"])
            .arg(certificate)
            .arg(&target),
        "failed to install the temporary broker CA (sudo is required)",
    )?;
    if let Err(error) = run(
        Command::new("sudo").arg(update_command),
        "failed to refresh the Linux trust store",
    ) {
        let _ = Command::new("sudo")
            .args(["rm", "-f"])
            .arg(&target)
            .status();
        return Err(error);
    }
    Ok(TemporaryCaTrust {
        cleanup: Cleanup::Linux {
            path: target,
            update_command,
        },
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn install_platform(_certificate: &Path, _subject: &str) -> Result<TemporaryCaTrust> {
    bail!("--trust-broker-ca is not supported on this platform")
}

fn run(command: &mut Command, error: &str) -> Result<()> {
    let status = command.status().with_context(|| error.to_owned())?;
    if status.success() {
        Ok(())
    } else {
        bail!(error.to_owned())
    }
}
