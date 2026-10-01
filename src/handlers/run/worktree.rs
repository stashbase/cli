//! Host-side lifecycle for `agent run --worktree`: create a per-session git
//! worktree, and after the (sandboxed) run, undo anything the agent could
//! have done to the shared git dir that would bite the host later.
//!
//! The agent can write the repo's common git dir (it must, to commit), so
//! the files host git executes or acts on are kept out of its reach: the
//! native backend adds `native_protected_paths` to the run's deny-list, the
//! Docker backend mounts them read-only (`docker_sandbox::append_git_mounts`)
//! and never mounts the user's checkout at all. Two pointer files
//! must stay writable for git to work at all: the worktree's own `.git`
//! file and its admin dir's `commondir`. Either can be redirected to an
//! attacker-controlled git dir whose config runs code (`core.fsmonitor`)
//! the next time host git touches the worktree — so `restore_pointers`
//! rewrites both from values captured here before anything on the host
//! runs git there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub(crate) struct RunWorktree {
    pub repo_root: PathBuf,
    pub common_dir: PathBuf,
    /// The worktree's root on the host.
    pub path: PathBuf,
    /// `<common_dir>/worktrees/<name>`: this worktree's HEAD/index/logs.
    pub admin_dir: PathBuf,
    /// Where the agent starts: `path` plus the caller's cwd relative to the repo root.
    pub workdir: PathBuf,
    pub branch: String,
    pointer_file: String,
    commondir_file: String,
    /// Every ref except `refs/heads/<branch>`, by full name → object id.
    ref_snapshot: BTreeMap<String, String>,
    /// The caller's checkout had uncommitted changes, which the worktree
    /// (created from `HEAD`) does not carry over.
    pub source_was_dirty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WorktreeOutcome {
    Removed,
    Kept,
}

/// `std::fs::canonicalize`, but without Windows' verbatim `\\?\` prefix
/// (`\\?\C:\repo` → `C:\repo`, `\\?\UNC\server\share` →
/// `\\server\share`): git can't handle verbatim paths ("could not create
/// leading directories of '//?/C:/…'"), and every path here ends up in a
/// git command or is compared with one git printed. Elsewhere it is plain
/// `canonicalize`.
pub(crate) fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{rest}")));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            if rest.as_bytes().get(1) == Some(&b':') {
                return Ok(PathBuf::from(rest));
            }
        }
    }
    Ok(path)
}

/// Runs `git -C dir ...` with no inherited `GIT_DIR`/`GIT_WORK_TREE`/
/// `GIT_COMMON_DIR` that could redirect it to another repository.
pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Where agent worktrees go by default, relative to the repo root: inside
/// the repo so they show up in the user's IDE next to the project, under
/// the repo's existing `.stashbase/` dir (which also holds committed files
/// like `.stashbase/agents/*.toml`, so only this subdirectory is ignored).
pub(crate) const REPO_WORKTREES_DIR: &str = ".stashbase/worktrees";

/// Words per generated worktree name: three words from the passphrase list
/// give ~2.4M combinations, and `create_named_run_worktree` retries on the
/// rare collision.
const NAME_WORDS: u8 = 3;
/// Namespace for agent branches, so they group together, are easy to list
/// or clean up (`git branch --list 'stashbase/*'`), never clash with the
/// user's own branch names, and match the `.stashbase/worktrees/` path.
pub(crate) const BRANCH_PREFIX: &str = "stashbase/";
const NAME_ATTEMPTS: usize = 5;

/// Like `create_run_worktree`, but names the worktree and its branch with a
/// random readable passphrase (`amber-river-storm` →
/// `.stashbase/worktrees/amber-river-storm` on `stashbase/amber-river-storm`),
/// retrying with a new name if that path or branch already exists.
pub(crate) fn create_named_run_worktree(
    cwd: &Path,
    worktrees_root: Option<&Path>,
) -> Result<RunWorktree, String> {
    let mut last_error = String::new();
    for _ in 0..NAME_ATTEMPTS {
        let name = crate::handlers::generate::passphrase::generate_passphrase(NAME_WORDS, "-");
        match create_run_worktree(cwd, &name, worktrees_root) {
            Ok(worktree) => return Ok(worktree),
            Err(error) if error.contains("already exists") => last_error = error,
            Err(error) => return Err(error),
        }
    }
    Err(format!(
        "could not find a free worktree name after {NAME_ATTEMPTS} attempts: {last_error}"
    ))
}

/// Prefix of the `git worktree lock` reason a run sets on its worktree for
/// as long as it runs, so `git worktree remove` — and therefore `agent
/// worktrees merge`/`clean` — never deletes a worktree an agent is still
/// working in. The reason also records the owning process (pid plus start
/// time, to survive pid reuse) so a lock left by a killed run can be told
/// apart from a live one.
pub(crate) const LOCK_REASON_PREFIX: &str = "stashbase run in progress";

fn lock_reason() -> String {
    let pid = std::process::id();
    let started = crate::handlers::agent::sessions::process_start_time(pid).unwrap_or_default();
    format!("{LOCK_REASON_PREFIX}; pid={pid}; started={started}")
}

/// Who holds a worktree lock, judged from its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunLock {
    /// A `stashbase` run that is still alive.
    Running,
    /// A `stashbase` run that no longer exists (killed before cleanup).
    Stale,
    /// Locked by someone else (e.g. by hand with `git worktree lock`).
    Other(String),
}

pub(crate) fn classify_lock(reason: &str) -> RunLock {
    let Some(details) = reason.strip_prefix(LOCK_REASON_PREFIX) else {
        return RunLock::Other(reason.to_owned());
    };
    let field = |name: &str| {
        details
            .split("; ")
            .find_map(|part| part.strip_prefix(&format!("{name}=")))
            .map(str::to_owned)
    };
    let Some(pid) = field("pid").and_then(|pid| pid.parse::<u32>().ok()) else {
        return RunLock::Stale;
    };
    if !process_exists(pid) {
        return RunLock::Stale;
    }
    // The pid exists; it is a *different* process only if both start
    // times are known and differ. When either can't be read (no `ps`,
    // restricted environment), assume it's still our run — wrongly
    // keeping a worktree is harmless, deleting a live one is not.
    let recorded = field("started").filter(|started| !started.is_empty());
    let current = crate::handlers::agent::sessions::process_start_time(pid).ok();
    match (recorded, current) {
        (Some(recorded), Some(current)) if recorded != current => RunLock::Stale,
        _ => RunLock::Running,
    }
}

/// Whether a process with this pid exists, without needing `ps`.
fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // Signal 0 checks existence/permission without delivering anything;
        // EPERM still means the process exists (owned by someone else).
        // SAFETY: `kill` with signal 0 has no side effects.
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        crate::handlers::agent::sessions::process_start_time(pid).is_ok()
    }
}

/// Checks that an existing agent worktree's git pointer files are exactly
/// what `git worktree add` writes, before any host git runs inside it.
///
/// During a run `restore_pointers` guarantees this, but a worktree left
/// behind by a killed `stashbase` process never got restored: its `.git`
/// file or `commondir` may still point at an agent-controlled git dir, or
/// its admin dir may hold an agent-written `config.worktree` — either can
/// make host git run agent code (`core.fsmonitor`). Returns why it is
/// unsafe, if it is.
pub(crate) fn verify_agent_worktree(path: &Path, common_dir: &Path) -> Result<(), String> {
    let pointer = path.join(".git");
    if !std::fs::symlink_metadata(&pointer).is_ok_and(|meta| meta.is_file()) {
        return Err(format!("{} is not a regular file", pointer.display()));
    }
    let contents = std::fs::read_to_string(&pointer)
        .map_err(|error| format!("cannot read {}: {error}", pointer.display()))?;
    let admin_dir = contents
        .strip_prefix("gitdir: ")
        .map(|rest| PathBuf::from(rest.trim_end_matches('\n')))
        .filter(|_| contents.ends_with('\n') && contents.lines().count() == 1)
        .ok_or_else(|| format!("{} has unexpected contents", pointer.display()))?;
    let worktrees_dir = canonicalize(common_dir.join("worktrees"))
        .map_err(|error| format!("cannot resolve the repository's worktrees dir: {error}"))?;
    let admin_dir = canonicalize(&admin_dir)
        .map_err(|_| format!("{} points at a missing git dir", pointer.display()))?;
    if admin_dir.parent() != Some(worktrees_dir.as_path()) {
        return Err(format!(
            "{} points outside this repository's worktree metadata",
            pointer.display()
        ));
    }

    let commondir_file = admin_dir.join("commondir");
    if !std::fs::symlink_metadata(&commondir_file).is_ok_and(|meta| meta.is_file()) {
        return Err(format!(
            "{} is not a regular file",
            commondir_file.display()
        ));
    }
    let commondir = std::fs::read_to_string(&commondir_file)
        .map_err(|error| format!("cannot read {}: {error}", commondir_file.display()))?;
    let resolved = canonicalize(admin_dir.join(commondir.trim_end_matches('\n')))
        .map_err(|_| format!("{} points at a missing directory", commondir_file.display()))?;
    let expected = canonicalize(common_dir)
        .map_err(|error| format!("cannot resolve the repository's git dir: {error}"))?;
    if resolved != expected {
        return Err(format!(
            "{} points at another git dir",
            commondir_file.display()
        ));
    }

    let worktree_config = admin_dir.join("config.worktree");
    if std::fs::symlink_metadata(&worktree_config).is_ok() {
        return Err(format!(
            "{} was written during a run",
            worktree_config.display()
        ));
    }
    Ok(())
}

/// Adds `/<relative>/` to the repo's `info/exclude` (a local, uncommitted
/// ignore file) unless already there, so agent worktrees nested in the
/// checkout never show up in the user's `git status`.
fn ensure_excluded(common_dir: &Path, relative: &Path) -> Result<(), String> {
    let pattern = format!("/{}/", relative.to_string_lossy().trim_matches('/'));
    let info = common_dir.join("info");
    let exclude = info.join("exclude");
    let current = std::fs::read_to_string(&exclude).unwrap_or_default();
    if current.lines().any(|line| line.trim() == pattern) {
        return Ok(());
    }
    std::fs::create_dir_all(&info)
        .map_err(|error| format!("failed to create {}: {error}", info.display()))?;
    let separator = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(
        &exclude,
        format!("{current}{separator}# stashbase agent worktrees\n{pattern}\n"),
    )
    .map_err(|error| format!("failed to update {}: {error}", exclude.display()))
}

fn snapshot_refs(repo_root: &Path, own_branch: &str) -> Result<BTreeMap<String, String>, String> {
    let own = format!("refs/heads/{own_branch}");
    Ok(git(
        repo_root,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    )?
    .lines()
    .filter_map(|line| line.split_once(' '))
    .filter(|(name, _)| *name != own)
    .map(|(name, oid)| (name.to_owned(), oid.to_owned()))
    .collect())
}

/// `worktrees_root` of `None` means `<repo root>/REPO_WORKTREES_DIR`.
pub(crate) fn create_run_worktree(
    cwd: &Path,
    name: &str,
    worktrees_root: Option<&Path>,
) -> Result<RunWorktree, String> {
    let repo_root = git(cwd, &["rev-parse", "--show-toplevel"]).map_err(|_| {
        format!(
            "--worktree: {} is not inside a git repository",
            cwd.display()
        )
    })?;
    let repo_root = canonicalize(&repo_root)
        .map_err(|error| format!("failed to resolve {repo_root}: {error}"))?;
    let common_dir = git(
        &repo_root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common_dir = canonicalize(&common_dir)
        .map_err(|error| format!("failed to resolve {common_dir}: {error}"))?;
    let cwd = canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let relative = cwd
        .strip_prefix(&repo_root)
        .unwrap_or(Path::new(""))
        .to_path_buf();
    if git(&repo_root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        return Err(format!(
            "--worktree: {} has no commits yet; the agent worktree is created from HEAD",
            repo_root.display()
        ));
    }
    let worktrees_root = worktrees_root
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo_root.join(REPO_WORKTREES_DIR));
    // Before the dirty check, so worktrees from earlier runs don't count
    // as uncommitted changes.
    if let Ok(inside) = worktrees_root.strip_prefix(&repo_root) {
        ensure_excluded(&common_dir, inside)?;
    }
    let source_was_dirty = !git(&repo_root, &["status", "--porcelain"])?.is_empty();

    let branch = format!("{BRANCH_PREFIX}{name}");
    std::fs::create_dir_all(&worktrees_root)
        .map_err(|error| format!("failed to create {}: {error}", worktrees_root.display()))?;
    let path = worktrees_root.join(name);
    let path_str = path.to_string_lossy().into_owned();
    git(
        &repo_root,
        &["worktree", "add", "-q", "-b", &branch, &path_str, "HEAD"],
    )?;

    // From here on the worktree and branch exist, but the caller only gets
    // a `RunWorktree` (and with it cleanup) if every remaining step works —
    // so undo them here on failure rather than leave a locked orphan.
    let finish_setup = || -> Result<RunWorktree, String> {
        #[cfg(test)]
        if tests::FAIL_AFTER_ADD.with(std::cell::Cell::get) {
            return Err("injected failure after `git worktree add`".to_owned());
        }
        git(
            &repo_root,
            &["worktree", "lock", "--reason", &lock_reason(), &path_str],
        )?;
        let path = canonicalize(&path).unwrap_or_else(|_| path.clone());

        // `git worktree add` names the admin dir after the path's basename,
        // uniquified with a numeric suffix on collision — read it back from
        // the pointer file rather than assuming.
        let pointer_file = std::fs::read_to_string(path.join(".git"))
            .map_err(|error| format!("failed to read the worktree's .git file: {error}"))?;
        let admin_dir = PathBuf::from(
            pointer_file
                .trim()
                .strip_prefix("gitdir: ")
                .ok_or_else(|| "unexpected worktree .git file format".to_owned())?,
        );
        // Git writes this with forward slashes on Windows; resolve it so it
        // compares equal to the other (native) paths. `pointer_file` keeps
        // the exact bytes for `restore_pointers`.
        let admin_dir = canonicalize(&admin_dir).unwrap_or(admin_dir);
        let commondir_file = std::fs::read_to_string(admin_dir.join("commondir"))
            .map_err(|error| format!("failed to read the worktree's commondir: {error}"))?;
        let ref_snapshot = snapshot_refs(&repo_root, &branch)?;

        Ok(RunWorktree {
            workdir: path.join(&relative),
            repo_root: repo_root.clone(),
            common_dir: common_dir.clone(),
            path,
            admin_dir,
            branch: branch.clone(),
            pointer_file,
            commondir_file,
            ref_snapshot,
            source_was_dirty,
        })
    };
    finish_setup().map_err(|error| match rollback_worktree(&repo_root, &path_str, &branch) {
        Ok(()) => error,
        Err(rollback) => format!(
            "{error} (and removing the half-created worktree failed: {rollback}; remove {path_str} and branch {branch} by hand)"
        ),
    })
}

/// Undoes a `git worktree add` whose setup didn't complete. Runs git only
/// from the repo root — the agent never ran in the new worktree. `--force`
/// twice also removes it if it was already locked.
fn rollback_worktree(repo_root: &Path, path: &str, branch: &str) -> Result<(), String> {
    git(
        repo_root,
        &["worktree", "remove", "--force", "--force", path],
    )?;
    git(repo_root, &["branch", "-D", branch])?;
    Ok(())
}

/// Rewrites `file` to `expected` unless it is already exactly that regular
/// file. `symlink_metadata` so a symlink is replaced, never followed.
fn restore_file(file: &Path, expected: &str) -> Result<bool, String> {
    let intact = std::fs::symlink_metadata(file).is_ok_and(|meta| meta.is_file())
        && std::fs::read_to_string(file).is_ok_and(|contents| contents == expected);
    if intact {
        return Ok(false);
    }
    match std::fs::symlink_metadata(file) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(file),
        Ok(_) => std::fs::remove_file(file),
        Err(_) => Ok(()),
    }
    .map_err(|error| format!("failed to remove tampered {}: {error}", file.display()))?;
    std::fs::write(file, expected)
        .map_err(|error| format!("failed to restore {}: {error}", file.display()))?;
    Ok(true)
}

/// Removes `path` whatever it is (file, symlink or directory). Returns
/// whether anything was there.
fn remove_any(path: &Path) -> Result<bool, String> {
    let removed = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) => return Ok(false),
    };
    removed.map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
    Ok(true)
}

impl RunWorktree {
    /// Must run before any host-side git command in `self.path`. Returns
    /// whether anything had been tampered with.
    ///
    /// Also removes `<admin_dir>/config.worktree`, which a fresh worktree
    /// never has: when the user's repo sets `extensions.worktreeConfig`,
    /// git reads it as this worktree's config, so the agent could put
    /// `core.fsmonitor` or `core.hooksPath` there and run code the next time
    /// host git (including this cleanup) touches the worktree.
    pub fn restore_pointers(&self) -> Result<bool, String> {
        let pointer = restore_file(&self.path.join(".git"), &self.pointer_file)?;
        let commondir = restore_file(&self.admin_dir.join("commondir"), &self.commondir_file)?;
        let worktree_config = remove_any(&self.admin_dir.join("config.worktree"))?;
        Ok(pointer || commondir || worktree_config)
    }

    /// Puts every ref other than this run's own branch back where it was
    /// before the run: moved/deleted refs are reset, refs the agent created
    /// are deleted. Returns a human-readable line per change.
    pub fn restore_foreign_refs(&self) -> Result<Vec<String>, String> {
        let now = snapshot_refs(&self.repo_root, &self.branch)?;
        let mut changes = Vec::new();
        for (name, before) in &self.ref_snapshot {
            if now.get(name) != Some(before) {
                git(&self.repo_root, &["update-ref", name, before])?;
                changes.push(format!(
                    "restored {name} to {}",
                    &before[..before.len().min(12)]
                ));
            }
        }
        for name in now
            .keys()
            .filter(|name| !self.ref_snapshot.contains_key(*name))
        {
            git(&self.repo_root, &["update-ref", "-d", name])?;
            changes.push(format!("removed {name} created during the run"));
        }
        Ok(changes)
    }

    /// Paths the native backend must deny writes to for this run: the git
    /// files host git executes or acts on (`config`, `hooks/`, submodule
    /// configs, the main checkout's `HEAD`/`index`), other worktrees'
    /// metadata, and the user's own checkout.
    ///
    /// The checkout can't be denied as a whole: it usually contains both
    /// the common git dir (the agent must write objects and refs there)
    /// and, by default, the agent's own worktree. So everything *beside*
    /// the path down to those two is denied instead — which covers the
    /// user's files, committed `.stashbase/agents` profiles, and other
    /// agents' worktrees. Known gap of the native backend: the agent can
    /// still *create* new entries next to those paths.
    pub fn native_protected_paths(&self) -> Result<Vec<String>, String> {
        let mut paths = Vec::new();
        if self.path.starts_with(&self.repo_root) || self.common_dir.starts_with(&self.repo_root) {
            self.protect_checkout_dir(&self.repo_root, &mut paths)?;
        } else {
            paths.push(self.repo_root.clone());
        }

        let hooks = self.common_dir.join("hooks");
        std::fs::create_dir_all(&hooks)
            .map_err(|error| format!("failed to create {}: {error}", hooks.display()))?;
        for name in [
            "config",
            "hooks",
            "modules",
            "config.worktree",
            "HEAD",
            "index",
        ] {
            let path = self.common_dir.join(name);
            if path.exists() {
                paths.push(path);
            }
        }
        if let Ok(entries) = std::fs::read_dir(self.common_dir.join("worktrees")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path != self.admin_dir {
                    paths.push(path);
                }
            }
        }
        Ok(paths
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect())
    }

    /// Denies every entry of `dir` except the ones leading to this run's
    /// worktree or the common git dir, recursing into those. The worktree
    /// and the git dir themselves are left out (the git dir gets its own
    /// targeted rules).
    fn protect_checkout_dir(&self, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir)
            .map_err(|error| format!("failed to list {}: {error}", dir.display()))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("failed to list {}: {error}", dir.display()))?
                .path();
            if path == self.path || path == self.common_dir {
                continue;
            }
            if self.path.starts_with(&path) || self.common_dir.starts_with(&path) {
                self.protect_checkout_dir(&path, paths)?;
            } else {
                paths.push(path);
            }
        }
        Ok(())
    }

    /// Removes the worktree if it has no uncommitted or untracked changes
    /// (the branch is kept either way); otherwise leaves it for the user.
    /// Caller must have run `restore_pointers` first.
    pub fn finish(&self) -> Result<WorktreeOutcome, String> {
        // The run is over either way: release the lock so the user (and
        // `agent worktrees merge`/`clean`) can remove the worktree.
        let path = self.path.to_string_lossy().into_owned();
        if let Err(error) = git(&self.repo_root, &["worktree", "unlock", &path]) {
            if !error.contains("is not locked") {
                return Err(error);
            }
        }
        if !git(&self.path, &["status", "--porcelain"])?.is_empty() {
            return Ok(WorktreeOutcome::Kept);
        }
        git(&self.repo_root, &["worktree", "remove", &path])?;
        Ok(WorktreeOutcome::Removed)
    }
}

/// Throwaway-repo helpers shared by the worktree tests here and the
/// sandbox tests in `subprocess` and `entry`.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A throwaway repo with one commit, a `sub/` dir and an extra branch
    /// `other`, plus an empty worktrees root — under the system temp dir,
    /// never the real project.
    pub(crate) fn fixture() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "stashbase-wt-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("sub/f"), "x").unwrap();
        git(&base, &["init", "-q", "-b", "main", "repo"]);
        // Code under test commits (merges) without the env identity the
        // `git` helper sets, so give the repo its own.
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "init"]);
        git(&repo, &["branch", "other"]);
        let repo = super::canonicalize(repo).unwrap();
        (repo, base.join("worktrees"))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{fixture, git};
    use super::*;

    thread_local! {
        /// Makes `create_run_worktree` fail right after `git worktree add`.
        pub(super) static FAIL_AFTER_ADD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    #[test]
    fn rollback_removes_an_already_locked_worktree() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_locked_rollback", Some(&root)).unwrap();
        let path = wt.path.to_string_lossy().into_owned();
        rollback_worktree(&repo, &path, &wt.branch).unwrap();
        assert!(!wt.path.exists());
        assert!(git(&repo, &["branch", "--list", &wt.branch]).is_empty());
    }

    #[test]
    fn failed_setup_after_add_removes_the_worktree_and_branch() {
        let (repo, root) = fixture();
        FAIL_AFTER_ADD.with(|fail| fail.set(true));
        let error = create_run_worktree(&repo, "ags_rollback", Some(&root)).unwrap_err();
        FAIL_AFTER_ADD.with(|fail| fail.set(false));

        assert!(error.contains("injected failure"), "{error}");
        assert!(!root.join("ags_rollback").exists(), "worktree dir removed");
        assert!(git(&repo, &["branch", "--list", "stashbase/ags_rollback"]).is_empty());
        let listing = git(&repo, &["worktree", "list", "--porcelain"]);
        assert!(
            !listing.contains("ags_rollback"),
            "not registered: {listing}"
        );

        // The name is free again.
        create_run_worktree(&repo, "ags_rollback", Some(&root)).unwrap();
    }

    #[test]
    fn creates_worktree_on_session_branch() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_a", Some(&root)).unwrap();
        assert_eq!(wt.branch, "stashbase/ags_a");
        assert_eq!(
            git(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "stashbase/ags_a"
        );
        assert_eq!(wt.admin_dir, wt.common_dir.join("worktrees").join("ags_a"));
    }

    #[test]
    fn relative_subdir_is_preserved() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo.join("sub"), "ags_b", Some(&root)).unwrap();
        assert_eq!(wt.workdir, wt.path.join("sub"));
    }

    #[test]
    fn dirty_source_is_reported() {
        let (repo, root) = fixture();
        std::fs::write(repo.join("sub/f"), "changed").unwrap();
        let wt = create_run_worktree(&repo, "ags_c", Some(&root)).unwrap();
        assert!(wt.source_was_dirty);
        assert_eq!(std::fs::read_to_string(wt.path.join("sub/f")).unwrap(), "x");
    }

    #[test]
    fn rejects_non_git_directory() {
        let dir = std::env::temp_dir().join(format!(
            "stashbase-wt-nogit-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let error = create_run_worktree(&dir, "ags_d", Some(&dir.join("w"))).unwrap_err();
        assert!(error.contains("not inside a git repository"), "{error}");
    }

    #[test]
    fn restore_pointers_undoes_tampering() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_e", Some(&root)).unwrap();
        std::fs::write(wt.path.join(".git"), "gitdir: /evil\n").unwrap();
        std::fs::write(wt.admin_dir.join("commondir"), "/evil\n").unwrap();
        assert!(wt.restore_pointers().unwrap(), "tampering must be reported");
        assert_eq!(
            git(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "stashbase/ags_e"
        );
        assert!(
            !wt.restore_pointers().unwrap(),
            "second call finds nothing to fix"
        );
    }

    #[test]
    fn restore_pointers_removes_an_agent_written_worktree_config() {
        let (repo, root) = fixture();
        git(&repo, &["config", "extensions.worktreeConfig", "true"]);
        let wt = create_run_worktree(&repo, "ags_wc", Some(&root)).unwrap();
        let marker = root.join("fsmonitor-ran");
        std::fs::write(
            wt.admin_dir.join("config.worktree"),
            format!(
                "[core]\n\tfsmonitor = touch '{}'; false\n",
                marker.display()
            ),
        )
        .unwrap();

        assert!(
            wt.restore_pointers().unwrap(),
            "agent-written config must be reported"
        );
        assert!(!wt.admin_dir.join("config.worktree").exists());
        git(&wt.path, &["status", "--porcelain"]);
        assert!(
            !marker.exists(),
            "host git must not run the agent's fsmonitor"
        );
    }

    #[test]
    fn restore_pointers_replaces_a_symlinked_pointer() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_f", Some(&root)).unwrap();
        std::fs::remove_file(wt.path.join(".git")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", wt.path.join(".git")).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir(wt.path.join(".git")).unwrap();
        assert!(wt.restore_pointers().unwrap());
        assert!(std::fs::symlink_metadata(wt.path.join(".git"))
            .unwrap()
            .is_file());
    }

    #[test]
    fn foreign_refs_are_restored_and_own_branch_is_not() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_g", Some(&root)).unwrap();
        let other_before = git(&repo, &["rev-parse", "other"]);
        std::fs::write(wt.path.join("n"), "n").unwrap();
        git(&wt.path, &["add", "n"]);
        git(&wt.path, &["commit", "-qm", "agent"]);
        let agent_tip = git(&wt.path, &["rev-parse", "HEAD"]);
        git(&wt.path, &["update-ref", "refs/heads/other", "HEAD"]);
        git(&wt.path, &["branch", "brand-new"]);
        wt.restore_pointers().unwrap();
        let changed = wt.restore_foreign_refs().unwrap();
        assert_eq!(git(&repo, &["rev-parse", "other"]), other_before);
        assert_eq!(git(&repo, &["rev-parse", "stashbase/ags_g"]), agent_tip);
        assert!(changed.iter().any(|c| c.contains("refs/heads/other")));
        assert!(changed.iter().any(|c| c.contains("refs/heads/brand-new")));
    }

    #[test]
    fn deleted_foreign_ref_is_recreated() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_h", Some(&root)).unwrap();
        let other_before = git(&repo, &["rev-parse", "other"]);
        git(&wt.path, &["branch", "-D", "other"]);
        wt.restore_pointers().unwrap();
        wt.restore_foreign_refs().unwrap();
        assert_eq!(git(&repo, &["rev-parse", "other"]), other_before);
    }

    #[test]
    fn native_protected_paths_cover_checkout_and_git_config_but_not_git_dir() {
        let (repo, root) = fixture();
        let other = create_run_worktree(&repo, "ags_k0", Some(&root)).unwrap();
        let wt = create_run_worktree(&repo, "ags_k", Some(&root)).unwrap();
        let paths = wt.native_protected_paths().unwrap();
        // Compare as paths, not strings: on Windows `join(".stashbase/agents")`
        // mixes separators that `Path` equality treats alike.
        let has = |p: &Path| paths.iter().any(|path| Path::new(path) == p);

        assert!(
            has(&repo.join("sub")),
            "checkout contents protected: {paths:?}"
        );
        assert!(
            !has(&wt.common_dir),
            "agent must be able to write objects/refs"
        );
        assert!(has(&wt.common_dir.join("config")));
        assert!(has(&wt.common_dir.join("hooks")));
        assert!(
            has(&wt.common_dir.join("HEAD")),
            "main checkout's branch protected"
        );
        assert!(has(&other.admin_dir), "other worktrees' metadata protected");
        assert!(!has(&wt.admin_dir), "own worktree metadata stays writable");
        assert!(!paths.iter().any(|p| Path::new(p).starts_with(&wt.path)));
    }

    #[test]
    fn foreign_ref_moved_after_gc_packs_refs_is_restored() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_gc", Some(&root)).unwrap();
        let other_before = git(&repo, &["rev-parse", "other"]);
        git(&wt.path, &["commit", "-q", "--allow-empty", "-m", "agent"]);
        // Pack every ref first, so `other` lives only in `packed-refs` when
        // the agent moves it, then gc again for good measure — the layout
        // that lost every branch in the feasibility spike.
        git(&wt.path, &["pack-refs", "--all"]);
        git(&wt.path, &["update-ref", "refs/heads/other", "HEAD"]);
        git(&wt.path, &["gc", "-q"]);
        wt.restore_pointers().unwrap();
        let changes = wt.restore_foreign_refs().unwrap();
        assert!(changes
            .iter()
            .any(|change| change.contains("refs/heads/other")));
        assert_eq!(git(&repo, &["rev-parse", "other"]), other_before);
        assert_eq!(
            git(&repo, &["log", "-1", "--format=%s", "stashbase/ags_gc"]),
            "agent"
        );
    }

    #[test]
    fn tags_moved_or_created_during_the_run_are_restored() {
        let (repo, root) = fixture();
        git(&repo, &["tag", "v1"]);
        let v1_before = git(&repo, &["rev-parse", "v1"]);
        let wt = create_run_worktree(&repo, "ags_tag", Some(&root)).unwrap();
        git(&wt.path, &["commit", "-q", "--allow-empty", "-m", "agent"]);
        git(&wt.path, &["tag", "-f", "v1"]);
        git(&wt.path, &["tag", "v2"]);
        wt.restore_pointers().unwrap();
        wt.restore_foreign_refs().unwrap();
        assert_eq!(git(&repo, &["rev-parse", "v1"]), v1_before);
        assert!(
            git(&repo, &["tag", "--list", "v2"]).is_empty(),
            "agent-created tag removed"
        );
    }

    #[test]
    fn reusing_a_session_id_fails_and_leaves_the_existing_worktree_alone() {
        let (repo, root) = fixture();
        let first = create_run_worktree(&repo, "ags_dup", Some(&root)).unwrap();
        std::fs::write(first.path.join("work"), "in progress").unwrap();
        let error = create_run_worktree(&repo, "ags_dup", Some(&root)).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(
            std::fs::read_to_string(first.path.join("work")).unwrap(),
            "in progress"
        );
        assert_eq!(
            git(&first.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "stashbase/ags_dup"
        );
    }

    #[test]
    fn repo_without_commits_is_rejected_clearly() {
        let base = std::env::temp_dir().join(format!(
            "stashbase-wt-empty-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&base).unwrap();
        git(&base, &["init", "-q", "-b", "main", "repo"]);
        let repo = base.join("repo");
        let error =
            create_run_worktree(&repo, "ags_empty", Some(&base.join("worktrees"))).unwrap_err();
        assert!(error.contains("no commits"), "{error}");
        assert!(!base.join("worktrees").join("ags_empty").exists());
    }

    #[test]
    fn starting_from_a_linked_worktree_protects_that_whole_checkout() {
        let (repo, root) = fixture();
        let users_worktree = root.parent().unwrap().join("users-wt");
        let users_worktree_str = users_worktree.to_string_lossy().into_owned();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &users_worktree_str,
            ],
        );
        let users_worktree = canonicalize(users_worktree).unwrap();

        let wt = create_run_worktree(&users_worktree, "ags_linked", Some(&root)).unwrap();
        assert_eq!(wt.repo_root, users_worktree);
        assert_eq!(git(&wt.path, &["log", "-1", "--format=%s"]), "init");
        let paths = wt.native_protected_paths().unwrap();
        assert!(
            paths.contains(&users_worktree.to_string_lossy().into_owned()),
            "the user's linked worktree is protected as a whole: {paths:?}"
        );
        assert!(!paths.iter().any(|p| Path::new(p) == wt.common_dir));
        assert!(!paths.iter().any(|p| Path::new(p).starts_with(&wt.path)));
    }

    #[test]
    fn agent_deleting_its_own_branch_does_not_break_cleanup() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_selfdel", Some(&root)).unwrap();
        git(
            &wt.path,
            &["update-ref", "-d", "refs/heads/stashbase/ags_selfdel"],
        );
        wt.restore_pointers().unwrap();
        assert!(
            wt.restore_foreign_refs().unwrap().is_empty(),
            "own branch is not a foreign ref"
        );
        // With its branch gone the worktree can't be proven clean, so it is
        // kept for the user rather than removed.
        assert_eq!(wt.finish().unwrap(), WorktreeOutcome::Kept);
        assert!(wt.path.exists());
    }

    #[test]
    fn default_location_is_inside_the_repo_and_hidden_from_git_status() {
        let (repo, _root) = fixture();
        std::fs::create_dir_all(repo.join(".stashbase/agents")).unwrap();
        std::fs::write(repo.join(".stashbase/agents/coding.toml"), "").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "profile"]);

        let wt = create_run_worktree(&repo, "ags_in", None).unwrap();
        assert_eq!(wt.path, repo.join(".stashbase/worktrees/ags_in"));
        assert!(!wt.source_was_dirty);
        assert!(
            git(&repo, &["status", "--porcelain"]).is_empty(),
            "worktree hidden"
        );

        // Only the worktrees dir is ignored: committed profiles stay tracked.
        std::fs::write(repo.join(".stashbase/agents/coding.toml"), "changed").unwrap();
        assert!(git(&repo, &["status", "--porcelain"]).contains(".stashbase/agents/coding.toml"));

        // A second run adds no duplicate exclude line.
        create_run_worktree(&repo, "ags_in2", None).unwrap();
        let exclude = std::fs::read_to_string(wt.common_dir.join("info/exclude")).unwrap();
        assert_eq!(exclude.matches("/.stashbase/worktrees/").count(), 1);
    }

    #[test]
    fn native_protection_covers_profiles_and_sibling_worktrees_but_not_own_path() {
        let (repo, _root) = fixture();
        std::fs::create_dir_all(repo.join(".stashbase/agents")).unwrap();
        let sibling = create_run_worktree(&repo, "ags_sib", None).unwrap();
        let wt = create_run_worktree(&repo, "ags_own", None).unwrap();
        let paths = wt.native_protected_paths().unwrap();
        // Compare as paths, not strings: on Windows `join(".stashbase/agents")`
        // mixes separators that `Path` equality treats alike.
        let has = |p: &Path| paths.iter().any(|path| Path::new(path) == p);

        assert!(
            has(&repo.join(".stashbase/agents")),
            "agent can't edit its own profile"
        );
        assert!(
            has(&sibling.path),
            "agent can't touch another agent's worktree"
        );
        assert!(has(&repo.join("sub")));
        assert!(
            !paths.iter().any(|p| wt.path.starts_with(p)),
            "own worktree writable: {paths:?}"
        );
        assert!(!paths.iter().any(|p| Path::new(p).starts_with(&wt.path)));
        assert!(!has(&wt.common_dir));
    }

    #[test]
    fn named_worktree_uses_a_readable_passphrase_for_path_and_branch() {
        let (repo, root) = fixture();
        let wt = create_named_run_worktree(&repo, Some(&root)).unwrap();
        let name = wt.path.file_name().unwrap().to_string_lossy().into_owned();
        let words: Vec<&str> = name.split('-').collect();
        assert_eq!(words.len(), 3, "{name}");
        assert!(words
            .iter()
            .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase())));
        assert_eq!(wt.branch, format!("stashbase/{name}"));
        assert_eq!(
            git(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            wt.branch
        );
    }

    #[test]
    fn verify_agent_worktree_accepts_fresh_and_rejects_tampered_pointers() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_verify", Some(&root)).unwrap();
        verify_agent_worktree(&wt.path, &wt.common_dir).unwrap();

        std::fs::write(wt.path.join(".git"), "gitdir: /tmp/evil\n").unwrap();
        assert!(verify_agent_worktree(&wt.path, &wt.common_dir).is_err());
        wt.restore_pointers().unwrap();
        verify_agent_worktree(&wt.path, &wt.common_dir).unwrap();

        std::fs::write(wt.admin_dir.join("commondir"), "/tmp\n").unwrap();
        assert!(verify_agent_worktree(&wt.path, &wt.common_dir).is_err());
        wt.restore_pointers().unwrap();

        std::fs::write(wt.admin_dir.join("config.worktree"), "[core]\n").unwrap();
        let error = verify_agent_worktree(&wt.path, &wt.common_dir).unwrap_err();
        assert!(error.contains("config.worktree"), "{error}");
    }

    #[test]
    fn worktree_is_locked_by_this_run_until_finish() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_lock", Some(&root)).unwrap();
        let listing = git(&repo, &["worktree", "list", "--porcelain"]);
        let reason = listing
            .lines()
            .find_map(|line| line.strip_prefix("locked "))
            .expect("worktree locked during the run");
        assert_eq!(classify_lock(reason), RunLock::Running);
        let path = wt.path.to_string_lossy().into_owned();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["worktree", "remove", &path])
                .output()
                .unwrap()
                .status
                .code()
                != Some(0),
            "git refuses to remove a running agent's worktree"
        );

        wt.restore_pointers().unwrap();
        wt.finish().unwrap();
        assert!(
            !wt.path.exists(),
            "finish unlocks, then removes the clean worktree"
        );
    }

    #[test]
    fn lock_of_a_dead_run_is_stale_and_foreign_locks_are_other() {
        assert_eq!(
            classify_lock(&format!(
                "{LOCK_REASON_PREFIX}; pid=999999; started=Thu Jan  1 00:00:00 1970"
            )),
            RunLock::Stale
        );
        let me = std::process::id();
        if crate::handlers::agent::sessions::process_start_time(me).is_ok() {
            assert_eq!(
                classify_lock(&format!(
                    "{LOCK_REASON_PREFIX}; pid={me}; started=not-my-start-time"
                )),
                RunLock::Stale,
                "a reused pid with a different start time is not our run"
            );
        }
        assert_eq!(
            classify_lock(&format!("{LOCK_REASON_PREFIX}; pid={me}; started=")),
            RunLock::Running,
            "live pid with unknown start time is treated as running"
        );
        assert_eq!(
            classify_lock("on a USB drive"),
            RunLock::Other("on a USB drive".to_owned())
        );
    }

    #[test]
    fn finish_removes_clean_worktree_keeps_branch() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_i", Some(&root)).unwrap();
        wt.restore_pointers().unwrap();
        assert!(matches!(wt.finish().unwrap(), WorktreeOutcome::Removed));
        assert!(!wt.path.exists());
        git(&repo, &["rev-parse", "--verify", "stashbase/ags_i"]);
    }

    #[test]
    fn finish_keeps_dirty_worktree() {
        let (repo, root) = fixture();
        let wt = create_run_worktree(&repo, "ags_j", Some(&root)).unwrap();
        std::fs::write(wt.path.join("untracked"), "u").unwrap();
        wt.restore_pointers().unwrap();
        assert!(matches!(wt.finish().unwrap(), WorktreeOutcome::Kept));
        assert!(wt.path.exists());
    }
}
