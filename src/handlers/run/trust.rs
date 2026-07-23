//! Temporary OS trust-store integration for the proxy's per-session CA.
//!
//! This is deliberately opt-in: changing the host trust store is observable state.

use std::{path::Path, process::Command};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use base64::{engine::general_purpose::STANDARD, Engine};

#[cfg(target_os = "linux")]
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

pub struct TemporaryCaTrust {
    cleanup: Cleanup,
}

enum Cleanup {
    #[cfg(target_os = "macos")]
    Macos {
        keychain: String,
        fingerprint: String,
    },
    #[cfg(target_os = "windows")]
    Windows { fingerprint: String },
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
            Cleanup::Macos {
                keychain,
                fingerprint,
            } => {
                let _ = Command::new("security")
                    .args(["delete-certificate", "-Z", fingerprint, "-t", keychain])
                    .status();
            }
            #[cfg(target_os = "windows")]
            Cleanup::Windows { fingerprint } => {
                let _ = Command::new("certutil")
                    .args(["-user", "-delstore", "Root", fingerprint])
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

pub fn install(certificate: &Path) -> Result<TemporaryCaTrust> {
    install_platform(certificate)
}

#[cfg(target_os = "macos")]
fn install_platform(certificate: &Path) -> Result<TemporaryCaTrust> {
    let fingerprint = certificate_sha1_fingerprint(certificate)?;
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
        "failed to trust the temporary proxy CA in the login Keychain",
    )?;
    Ok(TemporaryCaTrust {
        cleanup: Cleanup::Macos {
            keychain,
            fingerprint,
        },
    })
}

#[cfg(target_os = "windows")]
fn install_platform(certificate: &Path) -> Result<TemporaryCaTrust> {
    let fingerprint = certificate_sha1_fingerprint(certificate)?;
    run(
        Command::new("certutil")
            .args(["-user", "-addstore", "Root"])
            .arg(certificate),
        "failed to trust the temporary proxy CA in the current-user Root store",
    )?;
    Ok(TemporaryCaTrust {
        cleanup: Cleanup::Windows { fingerprint },
    })
}

#[cfg(target_os = "linux")]
fn install_platform(certificate: &Path) -> Result<TemporaryCaTrust> {
    let (directory, update_command) = if Path::new("/usr/sbin/update-ca-certificates").exists()
        || Path::new("/usr/bin/update-ca-certificates").exists()
    {
        ("/usr/local/share/ca-certificates", "update-ca-certificates")
    } else if Path::new("/usr/bin/update-ca-trust").exists()
        || Path::new("/usr/sbin/update-ca-trust").exists()
    {
        ("/etc/pki/ca-trust/source/anchors", "update-ca-trust")
    } else {
        bail!("unsupported Linux trust store; install update-ca-certificates or update-ca-trust, or run without --trust-proxy-ca")
    };
    let target = Path::new(directory).join(format!("stashbase-proxy-{}.crt", std::process::id()));
    run(
        Command::new("sudo")
            .args(["install", "-m", "0644"])
            .arg(certificate)
            .arg(&target),
        "failed to install the temporary proxy CA (sudo is required)",
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
fn install_platform(_certificate: &Path) -> Result<TemporaryCaTrust> {
    bail!("--trust-proxy-ca is not supported on this platform")
}

/// Returns the SHA-1 thumbprint understood by macOS Keychain and Windows
/// certificate-store deletion commands. A fingerprint targets exactly the
/// certificate this run installed, unlike a subject name which can be absent,
/// shared, or unrelated to a remotely-issued CA.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn certificate_sha1_fingerprint(certificate: &Path) -> Result<String> {
    let pem = std::fs::read_to_string(certificate)
        .with_context(|| format!("failed to read proxy CA at {}", certificate.display()))?;
    let encoded = pem
        .lines()
        .filter(|line| !line.starts_with("---"))
        .collect::<String>();
    let der = STANDARD
        .decode(encoded)
        .context("proxy CA is not valid PEM")?;
    Ok(hex::encode(sha1(&der)))
}

/// Minimal SHA-1 implementation for certificate-store thumbprints. SHA-1 is
/// used only as the platform-required certificate identifier, never for trust
/// validation or integrity checks.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn sha1(input: &[u8]) -> [u8; 20] {
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = [
        0x6745_2301u32,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    for chunk in data.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) =
            (state[0], state[1], state[2], state[3], state[4]);
        for (index, word) in words.into_iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut output = [0u8; 20];
    for (index, word) in state.into_iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn run(command: &mut Command, error: &str) -> Result<()> {
    let status = command.status().with_context(|| error.to_owned())?;
    if status.success() {
        Ok(())
    } else {
        bail!(error.to_owned())
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::sha1;

    #[test]
    fn calculates_certificate_store_sha1_thumbprints() {
        assert_eq!(
            hex::encode(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }
}
