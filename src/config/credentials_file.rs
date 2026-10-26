//! Owner-only API key file, used when no system keyring can store the key
//! (headless Linux without Secret Service, a locked macOS Keychain over SSH).
//! An entry exists only while the latest write for that profile went here, so
//! it takes precedence over the keyring when reading.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CREDENTIALS_FILE_NAME: &str = "credentials.toml";
const CREDENTIALS_FILE_MODE: u32 = 0o600;

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialsFile {
    #[serde(default)]
    api_keys: BTreeMap<String, String>,
}

/// `credentials.toml` next to `config.toml`.
pub fn path() -> Result<PathBuf> {
    Ok(crate::config::config::get_config_path()?.with_file_name(CREDENTIALS_FILE_NAME))
}

fn read(path: &Path) -> Result<CredentialsFile> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CredentialsFile::default())
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Could not read {}", path.display()))
        }
    };
    fs::set_permissions(path, fs::Permissions::from_mode(CREDENTIALS_FILE_MODE))?;
    toml::from_str(&content).with_context(|| format!("Could not parse {}", path.display()))
}

fn write(path: &Path, credentials: &CredentialsFile) -> Result<()> {
    if credentials.api_keys.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        };
    }
    // Written in full to a 0600 sibling and renamed over the original, so the
    // key is never readable by others, not even for a moment.
    let temporary = path.with_extension("toml.tmp");
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(CREDENTIALS_FILE_MODE)
        .open(&temporary)
        .with_context(|| format!("Could not write {}", temporary.display()))?;
    file.write_all(toml::to_string(credentials)?.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path).with_context(|| format!("Could not write {}", path.display()))?;
    Ok(())
}

pub fn get(path: &Path, profile: &str) -> Result<Option<String>> {
    Ok(read(path)?
        .api_keys
        .remove(profile)
        .filter(|key| !key.is_empty()))
}

pub fn set(path: &Path, profile: &str, api_key: &str) -> Result<()> {
    let mut credentials = read(path)?;
    credentials
        .api_keys
        .insert(profile.to_owned(), api_key.to_owned());
    write(path, &credentials)
}

/// Returns whether the file held a key for `profile`.
pub fn remove(path: &Path, profile: &str) -> Result<bool> {
    let mut credentials = read(path)?;
    if credentials.api_keys.remove(profile).is_none() {
        return Ok(false);
    }
    write(path, &credentials)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stashbase-credentials-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(CREDENTIALS_FILE_NAME)
    }

    #[test]
    fn missing_file_has_no_keys() {
        let path = temp_path();
        assert_eq!(get(&path, "default").unwrap(), None);
        assert!(!remove(&path, "default").unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn stores_keys_per_profile_owner_only() {
        let path = temp_path();
        set(&path, "default", "key-a").unwrap();
        set(&path, "work", "key-b").unwrap();
        set(&path, "default", "key-c").unwrap();

        assert_eq!(get(&path, "default").unwrap().as_deref(), Some("key-c"));
        assert_eq!(get(&path, "work").unwrap().as_deref(), Some("key-b"));
        assert_eq!(get(&path, "other").unwrap(), None);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, CREDENTIALS_FILE_MODE);
        assert!(!path.with_extension("toml.tmp").exists());
    }

    #[test]
    fn removing_the_last_key_deletes_the_file() {
        let path = temp_path();
        set(&path, "default", "key-a").unwrap();
        set(&path, "work", "key-b").unwrap();

        assert!(remove(&path, "default").unwrap());
        assert_eq!(get(&path, "default").unwrap(), None);
        assert!(path.exists());
        assert!(remove(&path, "work").unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn reading_tightens_loose_permissions() {
        let path = temp_path();
        set(&path, "default", "key-a").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(get(&path, "default").unwrap().as_deref(), Some("key-a"));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, CREDENTIALS_FILE_MODE);
    }
}
