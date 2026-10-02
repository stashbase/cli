use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub install_id: Option<Uuid>,
    /// `Some(false)` is an explicit opt-out via `stashbase telemetry disable`.
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub notice_shown: bool,
}

impl State {
    /// Returns the install ID, generating one if absent. The bool is true
    /// when a new ID was generated and the state needs saving.
    pub fn ensure_install_id(&mut self) -> (Uuid, bool) {
        match self.install_id {
            Some(id) => (id, false),
            None => {
                let id = Uuid::new_v4();
                self.install_id = Some(id);
                (id, true)
            }
        }
    }
}

pub fn state_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "stashbase").map(|dirs| dirs.config_dir().join("telemetry.json"))
}

/// A missing or corrupt file yields the default state; telemetry must never
/// make the CLI fail.
pub fn load(path: &Path) -> State {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, state: &State) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    write_private(&tmp, &bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("stashbase-telemetry-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_loads_default() {
        let dir = temp_dir("missing");
        assert_eq!(load(&dir.join("telemetry.json")), State::default());
    }

    #[test]
    fn corrupt_file_loads_default() {
        let dir = temp_dir("corrupt");
        let path = dir.join("telemetry.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(load(&path), State::default());
    }

    #[test]
    fn round_trips_and_keeps_install_id_stable() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("telemetry.json");
        let mut state = State::default();
        let (id, created) = state.ensure_install_id();
        assert!(created);
        state.notice_shown = true;
        save(&path, &state).unwrap();

        let mut loaded = load(&path);
        let (same, created_again) = loaded.ensure_install_id();
        assert_eq!(same, id);
        assert!(!created_again);
        assert!(loaded.notice_shown);
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perms");
        let path = dir.join("telemetry.json");
        save(&path, &State::default()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn save_fails_when_parent_is_not_a_directory() {
        let dir = temp_dir("unwritable");
        let blocker = dir.join("file");
        std::fs::write(&blocker, b"x").unwrap();
        assert!(save(&blocker.join("telemetry.json"), &State::default()).is_err());
    }
}
