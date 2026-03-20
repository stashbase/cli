use std::process::Command;
#[cfg(target_os = "windows")]
use std::{fs, path::PathBuf};
#[cfg(target_os = "linux")]
use std::{io::Write, process::Stdio};

use anyhow::{anyhow, Result};
#[cfg(target_os = "windows")]
use directories::ProjectDirs;

const SERVICE: &str = "stashbase-cli";
const ACCOUNT: &str = "default";

#[cfg(target_os = "macos")]
pub fn set_api_key(api_key: &str) -> Result<()> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-a",
            ACCOUNT,
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
pub fn set_api_key(api_key: &str) -> Result<()> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            "Stashbase CLI API Key",
            "service",
            SERVICE,
            "account",
            ACCOUNT,
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
pub fn get_api_key() -> Result<Option<String>> {
    let output = Command::new("security")
        .args(["find-generic-password", "-a", ACCOUNT, "-s", SERVICE, "-w"])
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
pub fn get_api_key() -> Result<Option<String>> {
    let output = Command::new("secret-tool")
        .args(["lookup", "service", SERVICE, "account", ACCOUNT])
        .output()?;

    if output.status.success() {
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            Ok(None)
        } else {
            Ok(Some(key))
        }
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
pub fn delete_api_key() -> Result<()> {
    let output = Command::new("security")
        .args(["delete-generic-password", "-a", ACCOUNT, "-s", SERVICE])
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
pub fn delete_api_key() -> Result<()> {
    let output = Command::new("secret-tool")
        .args(["clear", "service", SERVICE, "account", ACCOUNT])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to delete API key from Secret Service: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "windows")]
fn windows_secret_file_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "stashbase")
        .ok_or_else(|| anyhow!("Could not find config directory."))?;
    let config_dir = dirs.config_dir();
    fs::create_dir_all(config_dir)?;
    Ok(config_dir.join("secure-api-key.txt"))
}

#[cfg(target_os = "windows")]
fn escape_ps_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "windows")]
pub fn set_api_key(api_key: &str) -> Result<()> {
    let path = windows_secret_file_path()?;
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
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to store API key in Windows DPAPI store: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "windows")]
pub fn get_api_key() -> Result<Option<String>> {
    let path = windows_secret_file_path()?;
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
    let path = windows_secret_file_path()?;
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path)?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn set_api_key(_api_key: &str) -> Result<()> {
    Err(anyhow!(
        "Secure API key storage is not implemented for this OS."
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn get_api_key() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn delete_api_key() -> Result<()> {
    Ok(())
}
