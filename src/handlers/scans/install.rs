use crate::utils::output::get_formatted_json_string;
use anyhow::{anyhow, bail, Context, Result};
use git2::Repository;
use std::{
    fs,
    path::{Path, PathBuf},
};

const STASHBASE_SCAN_START_MARKER: &str = "# >>> stashbase scan >>>";
const STASHBASE_SCAN_END_MARKER: &str = "# <<< stashbase scan <<<";

#[derive(Debug, Clone, Copy)]
pub enum HookType {
    PreCommit,
    PrePush,
}

impl HookType {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pre-commit" => Ok(Self::PreCommit),
            "pre-push" => Ok(Self::PrePush),
            _ => bail!("Invalid hook. Expected 'pre-commit' or 'pre-push'."),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PreCommit => "pre-commit",
            Self::PrePush => "pre-push",
        }
    }

    fn scan_mode(self) -> &'static str {
        match self {
            Self::PreCommit => "staged",
            Self::PrePush => "unpushed",
        }
    }
}

pub fn install_scan_hook(
    hook_type: HookType,
    file_path: Option<&str>,
    silent: bool,
    json_format: bool,
) -> Result<()> {
    let repo = Repository::discover(".")
        .map_err(|_| anyhow!("Not a git repository. Run this inside a git project."))?;
    let git_dir = repo.path();

    let hook_file_path = resolve_hook_file_path(git_dir, hook_type, file_path);
    let parent_dir = hook_file_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid hook path '{}'", hook_file_path.display()))?;
    fs::create_dir_all(parent_dir).with_context(|| {
        format!(
            "Failed to create hooks directory at '{}'",
            parent_dir.display()
        )
    })?;

    let hook_block = format!(
        "{start}\ncommand -v stashbase >/dev/null 2>&1 || {{\n  echo \"stashbase CLI not found. Skipping scan.\"\n  exit 1\n}}\n\nstashbase scan {mode} --silent --json || exit 1\n{end}\n",
        start = STASHBASE_SCAN_START_MARKER,
        mode = hook_type.scan_mode(),
        end = STASHBASE_SCAN_END_MARKER,
    );

    let mut was_already_installed = false;

    if hook_file_path.exists() {
        let existing = fs::read_to_string(&hook_file_path)
            .with_context(|| format!("Failed to read '{}'", hook_file_path.display()))?;

        if existing.contains(STASHBASE_SCAN_START_MARKER)
            && existing.contains(STASHBASE_SCAN_END_MARKER)
        {
            let updated = replace_existing_stashbase_block(&existing, &hook_block);
            if updated == existing {
                was_already_installed = true;
            } else {
                fs::write(&hook_file_path, updated)
                    .with_context(|| format!("Failed to write '{}'", hook_file_path.display()))?;
            }
        } else {
            let mut updated = existing;
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push('\n');
            updated.push_str(&hook_block);

            fs::write(&hook_file_path, updated)
                .with_context(|| format!("Failed to write '{}'", hook_file_path.display()))?;
        }
    } else {
        let new_content = format!("#!/bin/sh\n\n{hook_block}");
        fs::write(&hook_file_path, new_content)
            .with_context(|| format!("Failed to create '{}'", hook_file_path.display()))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&hook_file_path, permissions).with_context(|| {
            format!(
                "Failed to set executable permissions on '{}'",
                hook_file_path.display()
            )
        })?;
    }

    if !silent {
        if json_format {
            let message = if was_already_installed {
                format!("Stashbase scan already installed for {}", hook_type.name())
            } else {
                format!("Installed {} hook", hook_type.name())
            };
            let payload = serde_json::json!({
                "message": message,
                "hook": hook_type.name(),
                "already_installed": was_already_installed
            });
            println!("\n{}", get_formatted_json_string(&payload, false)?);
        } else {
            println!();
            if was_already_installed {
                println!(
                    "✔ Stashbase scan already installed for {}",
                    hook_type.name()
                );
            } else {
                println!("✔ Installed {} hook", hook_type.name());
            }

            println!("Tip: run 'stashbase scan staged' to test it");
        }
    }

    Ok(())
}

pub fn uninstall_scan_hook(
    hook_type: HookType,
    file_path: Option<&str>,
    silent: bool,
    json_format: bool,
) -> Result<()> {
    let repo = Repository::discover(".")
        .map_err(|_| anyhow!("Not a git repository. Run this inside a git project."))?;
    let git_dir = repo.path();

    let hook_file_path = resolve_hook_file_path(git_dir, hook_type, file_path);

    if !hook_file_path.exists() {
        if !silent {
            if json_format {
                let payload = serde_json::json!({
                    "message": format!("Stashbase scan is not installed for {}", hook_type.name()),
                    "hook": hook_type.name(),
                    "uninstalled": false
                });
                println!("\n{}", get_formatted_json_string(&payload, false)?);
            } else {
                println!();
                println!("✔ Stashbase scan is not installed for {}", hook_type.name());
            }
        }
        return Ok(());
    }

    let existing = fs::read_to_string(&hook_file_path)
        .with_context(|| format!("Failed to read '{}'", hook_file_path.display()))?;

    let Some(updated) = remove_existing_stashbase_block(&existing) else {
        if !silent {
            if json_format {
                let payload = serde_json::json!({
                    "message": format!("Stashbase scan is not installed for {}", hook_type.name()),
                    "hook": hook_type.name(),
                    "uninstalled": false
                });
                println!("\n{}", get_formatted_json_string(&payload, false)?);
            } else {
                println!();
                println!("✔ Stashbase scan is not installed for {}", hook_type.name());
            }
        }
        return Ok(());
    };

    let normalized = normalize_after_uninstall(updated);
    fs::write(&hook_file_path, normalized)
        .with_context(|| format!("Failed to write '{}'", hook_file_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&hook_file_path, permissions).with_context(|| {
            format!(
                "Failed to set executable permissions on '{}'",
                hook_file_path.display()
            )
        })?;
    }

    if !silent {
        if json_format {
            let payload = serde_json::json!({
                "message": format!("Uninstalled {} hook", hook_type.name()),
                "hook": hook_type.name(),
                "uninstalled": true
            });
            println!("\n{}", get_formatted_json_string(&payload, false)?);
        } else {
            println!();
            println!("✔ Uninstalled {} hook", hook_type.name());
        }
    }
    Ok(())
}

fn resolve_hook_file_path(git_dir: &Path, hook_type: HookType, file_path: Option<&str>) -> PathBuf {
    if let Some(custom_path) = file_path {
        PathBuf::from(custom_path)
    } else {
        git_dir.join("hooks").join(hook_type.name())
    }
}

fn replace_existing_stashbase_block(existing: &str, hook_block: &str) -> String {
    let Some(start) = existing.find(STASHBASE_SCAN_START_MARKER) else {
        return existing.to_string();
    };
    let Some(end_marker_start_rel) = existing[start..].find(STASHBASE_SCAN_END_MARKER) else {
        return existing.to_string();
    };
    let end_marker_start = start + end_marker_start_rel;
    let mut end = end_marker_start + STASHBASE_SCAN_END_MARKER.len();
    if existing[end..].starts_with('\n') {
        end += 1;
    }

    let mut replacement = String::new();
    replacement.push_str(&existing[..start]);
    replacement.push_str(hook_block);
    replacement.push_str(&existing[end..]);
    replacement
}

fn remove_existing_stashbase_block(existing: &str) -> Option<String> {
    let start = existing.find(STASHBASE_SCAN_START_MARKER)?;
    let end_marker_start_rel = existing[start..].find(STASHBASE_SCAN_END_MARKER)?;
    let end_marker_start = start + end_marker_start_rel;
    let mut end = end_marker_start + STASHBASE_SCAN_END_MARKER.len();
    if existing[end..].starts_with('\n') {
        end += 1;
    }

    let mut output = String::new();
    output.push_str(&existing[..start]);
    output.push_str(&existing[end..]);
    Some(output)
}

fn normalize_after_uninstall(content: String) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "#!/bin/sh\n".to_string();
    }
    if trimmed == "#!/bin/sh" {
        return "#!/bin/sh\n".to_string();
    }

    let mut normalized = content;
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{install_scan_hook, uninstall_scan_hook, HookType};
    use once_cell::sync::Lazy;
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::Command,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };
    static TEST_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn enter(path: &Path) -> Self {
            let original = env::current_dir().expect("failed to read current dir");
            env::set_current_dir(path).expect("failed to set current dir");
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    fn temp_dir() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock error")
            .as_nanos();
        let dir = env::temp_dir().join(format!("stashbase-install-hook-{now}"));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    fn init_git_repo(path: &Path) {
        let status = Command::new("git")
            .arg("init")
            .arg(path)
            .status()
            .expect("failed to execute git init");
        assert!(status.success(), "git init failed");
    }

    #[test]
    fn creates_new_pre_commit_hook() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        install_scan_hook(HookType::PreCommit, None, false, false).expect("install failed");

        let content = fs::read_to_string(dir.join(".git/hooks/pre-commit")).expect("read failed");
        assert!(content.contains("#!/bin/sh"));
        assert!(content.contains("stashbase scan staged --silent --json || exit 1"));
    }

    #[test]
    fn appends_block_when_hook_exists_without_markers() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        let hook_path = dir.join(".git/hooks/pre-commit");
        fs::write(&hook_path, "#!/bin/sh\necho custom\n").expect("seed failed");

        install_scan_hook(HookType::PreCommit, None, false, false).expect("install failed");
        let content = fs::read_to_string(&hook_path).expect("read failed");

        assert!(content.contains("echo custom"));
        assert!(content.contains("# >>> stashbase scan >>>"));
    }

    #[test]
    fn is_idempotent_when_content_is_current() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        install_scan_hook(HookType::PreCommit, None, false, false).expect("first install failed");
        let hook_path = dir.join(".git/hooks/pre-commit");
        let first = fs::read_to_string(&hook_path).expect("first read failed");

        install_scan_hook(HookType::PreCommit, None, false, false).expect("second install failed");
        let second = fs::read_to_string(&hook_path).expect("second read failed");

        assert_eq!(first, second);
    }

    #[test]
    fn updates_existing_stashbase_block_when_template_changes() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        let legacy = "#!/bin/sh\n\n# >>> stashbase scan >>>\nstashbase scan staged || exit 1\n# <<< stashbase scan <<<\n";
        let hook_path = dir.join(".git/hooks/pre-commit");
        fs::write(&hook_path, legacy).expect("seed failed");

        install_scan_hook(HookType::PreCommit, None, false, false).expect("install failed");
        let content = fs::read_to_string(&hook_path).expect("read failed");

        assert!(content.contains("stashbase scan staged --silent --json || exit 1"));
        assert!(!content.contains("\nstashbase scan staged || exit 1\n"));
    }

    #[test]
    fn supports_custom_file_path() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        install_scan_hook(HookType::PreCommit, Some(".husky/pre-commit"), false, false)
            .expect("install failed");
        let content = fs::read_to_string(dir.join(".husky/pre-commit")).expect("read failed");

        assert!(content.contains("stashbase scan staged --silent --json || exit 1"));
    }

    #[test]
    fn discovers_repo_from_nested_directory() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let nested = dir.join("a/b/c");
        fs::create_dir_all(&nested).expect("mkdir failed");
        let _cwd = CwdGuard::enter(&nested);

        install_scan_hook(HookType::PreCommit, None, false, false).expect("install failed");
        assert!(dir.join(".git/hooks/pre-commit").exists());
    }

    #[test]
    fn uninstalls_only_stashbase_block_and_keeps_custom_content() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        let hook_path = dir.join(".git/hooks/pre-commit");
        let content = "#!/bin/sh\necho custom\n\n# >>> stashbase scan >>>\nstashbase scan staged --silent --json || exit 1\n# <<< stashbase scan <<<\n";
        fs::write(&hook_path, content).expect("seed failed");

        uninstall_scan_hook(HookType::PreCommit, None, false, false).expect("uninstall failed");
        let result = fs::read_to_string(&hook_path).expect("read failed");

        assert!(result.contains("echo custom"));
        assert!(!result.contains("# >>> stashbase scan >>>"));
    }

    #[test]
    fn uninstall_is_noop_when_not_installed() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        let hook_path = dir.join(".git/hooks/pre-commit");
        fs::write(&hook_path, "#!/bin/sh\necho custom\n").expect("seed failed");

        uninstall_scan_hook(HookType::PreCommit, None, false, false).expect("uninstall failed");
        let result = fs::read_to_string(&hook_path).expect("read failed");
        assert!(result.contains("echo custom"));
    }

    #[test]
    fn uninstall_writes_minimal_shell_when_file_becomes_empty() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        install_scan_hook(HookType::PreCommit, None, false, false).expect("install failed");
        uninstall_scan_hook(HookType::PreCommit, None, false, false).expect("uninstall failed");

        let content = fs::read_to_string(dir.join(".git/hooks/pre-commit")).expect("read failed");
        assert_eq!(content, "#!/bin/sh\n");
    }

    #[test]
    fn uninstall_supports_custom_file_path() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        install_scan_hook(HookType::PreCommit, Some(".husky/pre-commit"), false, false)
            .expect("install failed");
        uninstall_scan_hook(HookType::PreCommit, Some(".husky/pre-commit"), false, false)
            .expect("uninstall failed");

        let content = fs::read_to_string(dir.join(".husky/pre-commit")).expect("read failed");
        assert_eq!(content, "#!/bin/sh\n");
    }
}
