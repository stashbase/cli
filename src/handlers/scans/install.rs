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

/// Inside an agent sandbox the hook asks the Agent Proxy to run the scan on
/// the host, since the sandbox has neither the CLI nor an API key.
fn hook_block(hook_type: HookType) -> String {
    format!(
        r#"{start}
if [ -n "${STASHBASE_SCAN_BROKER_URL:-}" ]; then
  command -v curl >/dev/null 2>&1 || {{
    echo "curl not found in the agent sandbox. Cannot run the Stashbase scan."
    exit 1
  }}
  curl -sS --fail-with-body --noproxy '*' -X POST \
    -H "Authorization: Bearer ${STASHBASE_HOOK_BROKER_TOKEN:-}" \
    "$STASHBASE_SCAN_BROKER_URL/{mode}" || exit 1
elif [ "${STASHBASE_SANDBOX:-}" = "1" ]; then
  echo "Stashbase scan hook is not enabled for this agent run. Add allow_hooks = [\"secret_scan\"] to the agent profile."
  exit 1
else
  command -v stashbase >/dev/null 2>&1 || {{
    echo "stashbase CLI not found. Skipping scan."
    exit 1
  }}

  stashbase scan {mode} --silent --json || exit 1
fi
{end}
"#,
        start = STASHBASE_SCAN_START_MARKER,
        mode = hook_type.scan_mode(),
        end = STASHBASE_SCAN_END_MARKER,
    )
}

pub fn install_scan_hook(
    hook_type: HookType,
    file_path: Option<&str>,
    silent: bool,
    json_format: bool,
    print_leading_newline: bool,
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

    let hook_block = hook_block(hook_type);

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
            if print_leading_newline {
                println!();
            }
            println!("{}", get_formatted_json_string(&payload, false)?);
        } else {
            if print_leading_newline {
                println!();
            }
            if was_already_installed {
                println!(
                    "✔ Stashbase scan already installed for {}",
                    hook_type.name()
                );
            } else {
                println!("✔ Installed {} hook", hook_type.name());
            }
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
    use super::{hook_block, install_scan_hook, uninstall_scan_hook, HookType};
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

    #[cfg(unix)]
    fn run_block(hook_type: HookType, env: &[(&str, &str)], with_curl: bool) -> (i32, String) {
        run_script(hook_block(hook_type), env, with_curl)
    }

    #[cfg(unix)]
    fn run_script(script: String, env: &[(&str, &str)], with_curl: bool) -> (i32, String) {
        use std::os::unix::fs::PermissionsExt;

        let bin = temp_dir().join("bin");
        fs::create_dir_all(&bin).expect("failed to create bin dir");
        let stub = |name: &str, body: &str| {
            let path = bin.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("stub write failed");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod failed");
        };
        if with_curl {
            stub("curl", "echo \"curl $*\"; exit ${FAKE_CURL_EXIT:-0}");
        }
        stub("stashbase", "echo \"cli $*\"");

        // PATH holds only the stubs, so a real curl can never stand in for a
        // missing one; everything else the block uses is a shell builtin.
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env_clear()
            .env("PATH", &bin)
            .envs(env.iter().copied())
            .output()
            .expect("failed to run hook block");
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        (output.status.code().expect("hook was killed"), text)
    }

    #[cfg(unix)]
    const BROKER_ENV: [(&str, &str); 3] = [
        ("STASHBASE_SANDBOX", "1"),
        ("STASHBASE_SCAN_BROKER_URL", "http://h:1/__stashbase/scan"),
        ("STASHBASE_HOOK_BROKER_TOKEN", "t"),
    ];

    #[cfg(unix)]
    #[test]
    fn hook_uses_the_broker_inside_the_sandbox() {
        let (code, out) = run_block(HookType::PrePush, &BROKER_ENV, true);

        assert_eq!(code, 0, "{out}");
        assert!(out.contains("http://h:1/__stashbase/scan/unpushed"), "{out}");
        assert!(out.contains("Authorization: Bearer t"), "{out}");
        assert!(!out.contains("cli "), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn hook_fails_when_the_broker_reports_findings() {
        let mut env = BROKER_ENV.to_vec();
        env.push(("FAKE_CURL_EXIT", "22"));

        let (code, out) = run_block(HookType::PreCommit, &env, true);

        assert_eq!(code, 1, "{out}");
        assert!(out.contains("http://h:1/__stashbase/scan/staged"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn hook_in_sandbox_without_broker_names_the_profile_fix() {
        let (code, out) = run_block(HookType::PreCommit, &[("STASHBASE_SANDBOX", "1")], true);

        assert_eq!(code, 1, "{out}");
        assert!(out.contains("allow_hooks = [\"secret_scan\"]"), "{out}");
        assert!(!out.contains("stashbase CLI not found"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn hook_in_sandbox_without_curl_says_so() {
        let (code, out) = run_block(HookType::PreCommit, &BROKER_ENV, false);

        assert_eq!(code, 1, "{out}");
        assert!(out.contains("curl not found"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn hook_works_inside_an_existing_set_u_hook() {
        // `scan install` appends to existing hooks, which may use `set -u`.
        let script = format!("set -u\n{}", hook_block(HookType::PreCommit));

        let (code, out) = run_script(script.clone(), &[], true);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("cli scan staged --silent --json"), "{out}");

        let (code, out) = run_script(script, &BROKER_ENV, true);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("http://h:1/__stashbase/scan/staged"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn hook_outside_sandbox_runs_the_cli_as_before() {
        let (code, out) = run_block(HookType::PreCommit, &[], true);

        assert_eq!(code, 0, "{out}");
        assert!(out.contains("cli scan staged --silent --json"), "{out}");
    }

    #[test]
    fn creates_new_pre_commit_hook() {
        let _lock = test_lock();
        let dir = temp_dir();
        init_git_repo(&dir);
        let _cwd = CwdGuard::enter(&dir);

        install_scan_hook(HookType::PreCommit, None, false, false, true).expect("install failed");

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

        install_scan_hook(HookType::PreCommit, None, false, false, true).expect("install failed");
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

        install_scan_hook(HookType::PreCommit, None, false, false, true)
            .expect("first install failed");
        let hook_path = dir.join(".git/hooks/pre-commit");
        let first = fs::read_to_string(&hook_path).expect("first read failed");

        install_scan_hook(HookType::PreCommit, None, false, false, true)
            .expect("second install failed");
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

        install_scan_hook(HookType::PreCommit, None, false, false, true).expect("install failed");
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

        install_scan_hook(
            HookType::PreCommit,
            Some(".husky/pre-commit"),
            false,
            false,
            true,
        )
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

        install_scan_hook(HookType::PreCommit, None, false, false, true).expect("install failed");
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

        install_scan_hook(HookType::PreCommit, None, false, false, true).expect("install failed");
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

        install_scan_hook(
            HookType::PreCommit,
            Some(".husky/pre-commit"),
            false,
            false,
            true,
        )
        .expect("install failed");
        uninstall_scan_hook(HookType::PreCommit, Some(".husky/pre-commit"), false, false)
            .expect("uninstall failed");

        let content = fs::read_to_string(dir.join(".husky/pre-commit")).expect("read failed");
        assert_eq!(content, "#!/bin/sh\n");
    }
}
