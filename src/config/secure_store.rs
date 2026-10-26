#[cfg(target_os = "windows")]
use std::fs;
#[cfg(any(unix, target_os = "windows"))]
use std::path::PathBuf;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::{io::Write, process::Stdio};

#[cfg(unix)]
use crate::utils::output::ColorizeIfColoredOutput;
use anyhow::{anyhow, Result};
#[cfg(target_os = "windows")]
use directories::ProjectDirs;

const SERVICE: &str = "stashbase-cli";
const ACCOUNT: &str = "default";

/// Where `set_api_key_for_profile` put the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStorage {
    /// macOS Keychain, Secret Service, or the Windows DPAPI store.
    SystemStore,
    /// The owner-only credentials file, because the system store failed.
    #[cfg(unix)]
    File(PathBuf),
}

/// Tells the user, once at store time, that the key went to the credentials
/// file instead of the system keyring.
pub fn report_storage(storage: &KeyStorage) {
    #[cfg(unix)]
    if let KeyStorage::File(path) = storage {
        eprintln!(
            "{} No system keyring is available, so the API key was stored in {} (readable only by you).",
            "Note:".yellow_if_tty_stderr(),
            path.display()
        );
    }
    #[cfg(not(unix))]
    let _ = storage;
}

fn account_for_profile(profile: &str) -> String {
    if profile == ACCOUNT {
        ACCOUNT.to_owned()
    } else {
        format!("profile:{profile}")
    }
}

#[cfg(target_os = "macos")]
fn keyring_set(profile: &str, api_key: &str) -> Result<()> {
    let account = account_for_profile(profile);
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-a",
            &account,
            "-s",
            SERVICE,
            "-w",
            api_key,
        ])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to store API key in macOS Keychain: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
fn keyring_set(profile: &str, api_key: &str) -> Result<()> {
    let account = account_for_profile(profile);
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            "Stashbase CLI API Key",
            "service",
            SERVICE,
            "account",
            &account,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(api_key.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to store API key in Secret Service: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "macos")]
fn keyring_get(profile: &str) -> Result<Option<String>> {
    let account = account_for_profile(profile);
    let output = Command::new("security")
        .args(["find-generic-password", "-a", &account, "-s", SERVICE, "-w"])
        .output()?;

    if output.status.success() {
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            Ok(None)
        } else {
            Ok(Some(key))
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if stderr.contains("could not be found") {
            Ok(None)
        } else {
            Err(anyhow!(
                "Failed to read API key from macOS Keychain: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

#[cfg(target_os = "linux")]
fn keyring_get(profile: &str) -> Result<Option<String>> {
    let account = account_for_profile(profile);
    let output = match Command::new("secret-tool")
        .args(["lookup", "service", SERVICE, "account", &account])
        .output()
    {
        Ok(output) => output,
        // No `secret-tool` (a headless server): no keyring can hold a key.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    parse_linux_secret_tool_lookup_output(&output.stdout, &output.stderr, output.status.success())
}

#[cfg(target_os = "macos")]
fn keyring_delete(profile: &str) -> Result<()> {
    let account = account_for_profile(profile);
    let output = Command::new("security")
        .args(["delete-generic-password", "-a", &account, "-s", SERVICE])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if stderr.contains("could not be found") {
            Ok(())
        } else {
            Err(anyhow!(
                "Failed to delete API key from macOS Keychain: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

#[cfg(target_os = "linux")]
fn keyring_delete(profile: &str) -> Result<()> {
    let account = account_for_profile(profile);
    let output = match Command::new("secret-tool")
        .args(["clear", "service", SERVICE, "account", &account])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to delete API key from Secret Service: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Stores the key in the system keyring, or in the owner-only credentials
/// file when the keyring can't take it.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn set_api_key_for_profile(profile: &str, api_key: &str) -> Result<KeyStorage> {
    let path = super::credentials_file::path()?;
    match keyring_set(profile, api_key) {
        Ok(()) => {
            // Drop a key an earlier fallback left, so it doesn't shadow this one.
            super::credentials_file::remove(&path, profile)?;
            Ok(KeyStorage::SystemStore)
        }
        Err(keyring_error) => {
            super::credentials_file::set(&path, profile, api_key).map_err(|file_error| {
                anyhow!("{keyring_error}; the credentials file fallback failed too: {file_error}")
            })?;
            Ok(KeyStorage::File(path))
        }
    }
}

/// The credentials file is read first: it only holds a key while the latest
/// write for that profile went there.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn get_api_key_for_profile(profile: &str) -> Result<Option<String>> {
    if let Some(key) = super::credentials_file::get(&super::credentials_file::path()?, profile)? {
        return Ok(Some(key));
    }
    keyring_get(profile)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn delete_api_key() -> Result<()> {
    delete_api_key_for_profile(ACCOUNT)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn delete_api_key_for_profile(profile: &str) -> Result<()> {
    let removed_from_file =
        super::credentials_file::remove(&super::credentials_file::path()?, profile)?;
    match keyring_delete(profile) {
        // The key lived in the file because the keyring doesn't work here.
        Err(_) if removed_from_file => Ok(()),
        result => result,
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_secret_tool_lookup_output(
    stdout: &[u8],
    stderr: &[u8],
    success: bool,
) -> Result<Option<String>> {
    if success {
        let key = String::from_utf8_lossy(stdout).trim().to_string();
        if key.is_empty() {
            Ok(None)
        } else {
            Ok(Some(key))
        }
    } else {
        let stderr = String::from_utf8_lossy(stderr).trim().to_string();
        if stderr.is_empty() {
            Ok(None)
        } else {
            Err(anyhow!(
                "Failed to read API key from Secret Service: {}",
                stderr
            ))
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_secret_file_path(profile: &str) -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "stashbase")
        .ok_or_else(|| anyhow!("Could not find config directory."))?;
    let config_dir = dirs.config_dir();
    fs::create_dir_all(config_dir)?;
    if profile == ACCOUNT {
        Ok(config_dir.join("secure-api-key.txt"))
    } else {
        let encoded = hex::encode(profile.as_bytes());
        Ok(config_dir.join(format!("secure-api-key-profile-{encoded}.txt")))
    }
}

#[cfg(target_os = "windows")]
fn escape_ps_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "windows")]
pub fn set_api_key_for_profile(profile: &str, api_key: &str) -> Result<KeyStorage> {
    let path = windows_secret_file_path(profile)?;
    let path = escape_ps_single_quoted(path.to_string_lossy().as_ref());

    let script = format!(
        "$s = ConvertTo-SecureString -String $env:STASHBASE_API_KEY_PLAIN -AsPlainText -Force; \
         $e = ConvertFrom-SecureString -SecureString $s; \
         Set-Content -LiteralPath '{}' -Value $e -Encoding UTF8 -NoNewline",
        path
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .env("STASHBASE_API_KEY_PLAIN", api_key)
        .output()?;

    if output.status.success() {
        Ok(KeyStorage::SystemStore)
    } else {
        Err(anyhow!(
            "Failed to store API key in Windows DPAPI store: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "windows")]
pub fn get_api_key_for_profile(profile: &str) -> Result<Option<String>> {
    let path = windows_secret_file_path(profile)?;
    if !path.exists() {
        return Ok(None);
    }

    let path = escape_ps_single_quoted(path.to_string_lossy().as_ref());

    let script = format!(
        "$enc = Get-Content -LiteralPath '{}' -Raw; \
         if ([string]::IsNullOrWhiteSpace($enc)) {{ exit 0 }}; \
         $secure = ConvertTo-SecureString -String $enc; \
         $plain = [System.Net.NetworkCredential]::new('', $secure).Password; \
         Write-Output $plain",
        path
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()?;

    if output.status.success() {
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            Ok(None)
        } else {
            Ok(Some(key))
        }
    } else {
        Err(anyhow!(
            "Failed to read API key from Windows DPAPI store: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "windows")]
pub fn delete_api_key() -> Result<()> {
    delete_api_key_for_profile(ACCOUNT)
}

#[cfg(target_os = "windows")]
pub fn delete_api_key_for_profile(profile: &str) -> Result<()> {
    let path = windows_secret_file_path(profile)?;
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path)?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn set_api_key_for_profile(_profile: &str, _api_key: &str) -> Result<KeyStorage> {
    Err(anyhow!(
        "Secure API key storage is not implemented for this OS."
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn get_api_key_for_profile(_profile: &str) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn delete_api_key() -> Result<()> {
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn delete_api_key_for_profile(_profile: &str) -> Result<()> {
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_linux_secret_tool_lookup_output;

    #[test]
    fn linux_lookup_returns_none_when_secret_is_missing() {
        let result = parse_linux_secret_tool_lookup_output(b"", b"", false).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn linux_lookup_returns_error_when_secret_service_fails() {
        let err =
            parse_linux_secret_tool_lookup_output(b"", b"dbus unavailable", false).unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to read API key from Secret Service: dbus unavailable"));
    }
}
