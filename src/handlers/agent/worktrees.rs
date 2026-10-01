//! `stashbase agent worktrees list|merge|clean`: review and integrate the
//! work agents left on their `stashbase/<name>` branches.
//!
//! Every git command here runs from the user's own checkout, except
//! `git status` inside an agent worktree — and that only after
//! `verify_agent_worktree` has confirmed its pointer files are exactly what
//! git wrote. A worktree left behind by a killed run was never restored by
//! the run's own cleanup, so it is treated as untrusted until checked.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tabled::Tabled;

use crate::cmd::agent::{AgentWorktreesCleanCommand, AgentWorktreesMergeCommand};
use crate::handlers::run::worktree::{
    canonicalize, classify_lock, git, verify_agent_worktree, RunLock, BRANCH_PREFIX,
};
use crate::utils::output::{get_formatted_json_string, ColorizeIfColoredOutput};

/// The checkout the command runs in: agent branches are compared against,
/// and merged into, its current branch.
#[derive(Debug)]
pub(crate) struct Checkout {
    pub root: PathBuf,
    pub common_dir: PathBuf,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub(crate) enum WorktreeState {
    /// Worktree present with no uncommitted or untracked changes.
    Clean,
    /// Worktree present with uncommitted or untracked changes.
    Uncommitted,
    /// Only the branch is left (the worktree was removed after its run).
    NoWorktree,
    /// Pointer files don't match what git wrote: never run git in it.
    Unsafe(String),
    /// A `stashbase` run is still working in it: never remove it.
    Running,
    /// Locked by someone other than stashbase (`git worktree lock`).
    Locked(String),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentWorktree {
    pub name: String,
    pub branch: String,
    pub path: Option<PathBuf>,
    #[serde(flatten)]
    pub state: WorktreeState,
    /// Commits on the agent branch that the checkout's branch doesn't have.
    pub ahead: usize,
    /// Locked by a `stashbase` run that no longer exists; removal unlocks
    /// it first.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stale_lock: bool,
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    git(dir, args).map_err(|error| anyhow::anyhow!(error))
}

pub(crate) fn open_checkout(cwd: &Path) -> Result<Checkout> {
    let root = run_git(cwd, &["rev-parse", "--show-toplevel"])
        .map_err(|_| anyhow::anyhow!("{} is not inside a git repository", cwd.display()))?;
    let root = canonicalize(&root).with_context(|| format!("failed to resolve {root}"))?;
    let common_dir = run_git(
        &root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common_dir =
        canonicalize(&common_dir).with_context(|| format!("failed to resolve {common_dir}"))?;
    let branch = run_git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
    if branch
        .as_deref()
        .is_some_and(|branch| branch.starts_with(BRANCH_PREFIX))
    {
        bail!(
            "this is an agent worktree ({}); run `stashbase agent worktrees` from your own checkout",
            root.display()
        );
    }
    Ok(Checkout {
        root,
        common_dir,
        branch,
    })
}

struct ListedWorktree {
    branch: String,
    path: PathBuf,
    /// `Some` when locked; the reason may be empty.
    lock: Option<String>,
}

/// Every worktree git knows about that has a branch, from
/// `git worktree list --porcelain` (blank-line-separated blocks).
fn listed_worktrees(checkout: &Checkout) -> Result<Vec<ListedWorktree>> {
    let listing = run_git(&checkout.root, &["worktree", "list", "--porcelain"])?;
    let mut worktrees = Vec::new();
    for block in listing.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        let mut lock = None;
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(rest));
            } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
                branch = Some(rest.to_owned());
            } else if line == "locked" {
                lock = Some(String::new());
            } else if let Some(reason) = line.strip_prefix("locked ") {
                lock = Some(reason.to_owned());
            }
        }
        if let (Some(path), Some(branch)) = (path, branch) {
            worktrees.push(ListedWorktree { branch, path, lock });
        }
    }
    Ok(worktrees)
}

fn inspect_worktree(checkout: &Checkout, path: &Path) -> WorktreeState {
    if !path.exists() {
        return WorktreeState::NoWorktree;
    }
    if let Err(reason) = verify_agent_worktree(path, &checkout.common_dir) {
        return WorktreeState::Unsafe(reason);
    }
    match git(path, &["status", "--porcelain"]) {
        Ok(status) if status.is_empty() => WorktreeState::Clean,
        Ok(_) => WorktreeState::Uncommitted,
        Err(reason) => WorktreeState::Unsafe(reason),
    }
}

pub(crate) fn list_agent_worktrees(checkout: &Checkout) -> Result<Vec<AgentWorktree>> {
    let listed = listed_worktrees(checkout)?;
    let prefix = format!("refs/heads/{BRANCH_PREFIX}");
    let branches = run_git(
        &checkout.root,
        &["for-each-ref", "--format=%(refname:short)", &prefix],
    )?;
    let base = if checkout.branch.is_some() {
        "HEAD"
    } else {
        ""
    };
    let mut worktrees = Vec::new();
    for branch in branches.lines().filter(|line| !line.is_empty()) {
        let listed = listed.iter().find(|listed| listed.branch == branch);
        let path = listed.map(|listed| listed.path.clone());
        let lock = listed
            .and_then(|listed| listed.lock.as_deref())
            .map(classify_lock);
        let state = match (&path, &lock) {
            (None, _) => WorktreeState::NoWorktree,
            // Unsafe beats everything: whatever holds the lock, the user
            // must inspect it before git runs there.
            (Some(path), _)
                if path.exists() && verify_agent_worktree(path, &checkout.common_dir).is_err() =>
            {
                inspect_worktree(checkout, path)
            }
            (Some(_), Some(RunLock::Running)) => WorktreeState::Running,
            (Some(_), Some(RunLock::Other(reason))) => WorktreeState::Locked(reason.clone()),
            (Some(path), _) => inspect_worktree(checkout, path),
        };
        let ahead = if base.is_empty() {
            0
        } else {
            run_git(
                &checkout.root,
                &[
                    "rev-list",
                    "--count",
                    &format!("{base}..refs/heads/{branch}"),
                ],
            )?
            .parse()
            .unwrap_or(0)
        };
        worktrees.push(AgentWorktree {
            name: branch.trim_start_matches(BRANCH_PREFIX).to_owned(),
            branch: branch.to_owned(),
            path: path.filter(|path| path.exists()),
            state,
            ahead,
            stale_lock: lock == Some(RunLock::Stale),
        });
    }
    Ok(worktrees)
}

fn find<'a>(worktrees: &'a [AgentWorktree], name: &str) -> Result<&'a AgentWorktree> {
    let name = name.trim_start_matches(BRANCH_PREFIX);
    worktrees
        .iter()
        .find(|worktree| worktree.name == name)
        .with_context(|| {
            format!("no agent worktree named '{name}'; see `stashbase agent worktrees list`")
        })
}

/// Removes an agent's worktree (if any) and branch. `force` also discards
/// uncommitted changes and unmerged commits; an unsafe worktree is never
/// touched.
pub(crate) fn remove_agent_worktree(
    checkout: &Checkout,
    worktree: &AgentWorktree,
    force: bool,
) -> Result<()> {
    match &worktree.state {
        WorktreeState::Unsafe(reason) => bail!(
            "skipping {}: {reason}; inspect it by hand before running git there",
            worktree.name
        ),
        WorktreeState::Running => bail!(
            "skipping {}: an agent run is still working in it",
            worktree.name
        ),
        WorktreeState::Locked(reason) => bail!(
            "skipping {}: locked ({}); unlock it with `git worktree unlock` first",
            worktree.name,
            if reason.is_empty() {
                "no reason given"
            } else {
                reason
            }
        ),
        _ => {}
    }
    if let Some(path) = &worktree.path {
        let path = path.to_string_lossy();
        if worktree.stale_lock {
            run_git(&checkout.root, &["worktree", "unlock", &path])?;
        }
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path);
        run_git(&checkout.root, &args)?;
        // A Docker run's isolated-path volumes are keyed by the worktree
        // path; nothing else will ever use them. No-op without Docker.
        for error in crate::handlers::run::docker_sandbox::remove_isolated_path_volumes_under(
            Path::new(path.as_ref()),
        ) {
            eprintln!("warning: failed to remove isolated path volume {error}");
        }
    }
    let delete = if force || worktree.ahead == 0 {
        "-D"
    } else {
        "-d"
    };
    run_git(&checkout.root, &["branch", delete, &worktree.branch])?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MergeOutcome {
    /// `kept_running`: the agent is still working in the worktree, so it
    /// and the branch were kept even without `--keep`.
    Merged {
        commits: usize,
        into: String,
        kept_running: bool,
    },
    NothingToMerge,
}

pub(crate) fn merge_agent_worktree(
    checkout: &Checkout,
    name: &str,
    squash: bool,
    message: Option<&str>,
    keep: bool,
) -> Result<MergeOutcome> {
    if message.is_some_and(|message| message.trim().is_empty()) {
        bail!("the commit message must not be empty");
    }
    let into = checkout
        .branch
        .clone()
        .context("your checkout is on a detached HEAD; check out the branch to merge into first")?;
    let worktrees = list_agent_worktrees(checkout)?;
    let worktree = find(&worktrees, name)?;
    match &worktree.state {
        WorktreeState::Unsafe(reason) => bail!(
            "refusing to merge {}: {reason}; inspect it by hand first",
            worktree.name
        ),
        WorktreeState::Uncommitted => bail!(
            "{} has uncommitted changes; commit them in {} first",
            worktree.name,
            worktree
                .path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        ),
        WorktreeState::Locked(_)
        | WorktreeState::Running
        | WorktreeState::Clean
        | WorktreeState::NoWorktree => {}
    }
    // A running agent's committed work can be merged, but its worktree
    // must stay — it's still being worked in.
    let running = worktree.state == WorktreeState::Running;
    let keep = keep || running || matches!(worktree.state, WorktreeState::Locked(_));
    if worktree.ahead == 0 {
        if !keep {
            remove_agent_worktree(checkout, worktree, false)?;
        }
        return Ok(MergeOutcome::NothingToMerge);
    }
    if !run_git(
        &checkout.root,
        &["status", "--porcelain", "--untracked-files=no"],
    )?
    .is_empty()
    {
        bail!("your checkout has uncommitted changes; commit or stash them before merging");
    }

    let reference = format!("refs/heads/{}", worktree.branch);
    let merged = if squash {
        let message = match message {
            Some(message) => message.to_owned(),
            None => {
                let subjects = run_git(
                    &checkout.root,
                    &[
                        "log",
                        "--reverse",
                        "--format=- %s",
                        &format!("HEAD..{reference}"),
                    ],
                )?;
                format!("Squash branch '{}'\n\n{subjects}", worktree.branch)
            }
        };
        git(&checkout.root, &["merge", "--squash", &reference])
            .and_then(|_| git(&checkout.root, &["commit", "-q", "-m", &message]))
    } else {
        // Without a message, git writes its usual "Merge branch '…' [into
        // …]" (honouring settings like `merge.log`); passing the short
        // branch name rather than `refs/heads/…` keeps that wording.
        match message {
            Some(message) => git(
                &checkout.root,
                &["merge", "--no-ff", "-m", message, &worktree.branch],
            ),
            None => git(
                &checkout.root,
                &["merge", "--no-ff", "--no-edit", &worktree.branch],
            ),
        }
    };
    if let Err(error) = merged {
        bail!(
            "merging {} into {into} stopped: {error}\nResolve the conflicts and commit, or run `git merge --abort`; the agent's worktree and branch were kept.",
            worktree.branch
        );
    }
    if !keep {
        // Squashed commits aren't ancestors of the checkout, so the branch
        // needs a forced delete; its content is in the squash commit.
        remove_agent_worktree(checkout, worktree, squash)?;
    }
    Ok(MergeOutcome::Merged {
        commits: worktree.ahead,
        into,
        kept_running: running,
    })
}

/// What `clean` removes: without `all`, only agent work that is already in
/// the checkout's branch and has no uncommitted changes. Worktrees a run is
/// still using (or someone locked) are never candidates, even with `all`.
/// Unsafe worktrees are listed so the user sees them, but
/// `remove_agent_worktree` skips them.
pub(crate) fn clean_candidates(worktrees: Vec<AgentWorktree>, all: bool) -> Vec<AgentWorktree> {
    worktrees
        .into_iter()
        .filter(|worktree| {
            !matches!(
                worktree.state,
                WorktreeState::Running | WorktreeState::Locked(_)
            ) && (all
                || (worktree.ahead == 0
                    && matches!(
                        worktree.state,
                        WorktreeState::Clean | WorktreeState::NoWorktree
                    )))
        })
        .collect()
}

#[derive(Tabled)]
struct WorktreeRow {
    name: String,
    status: String,
    #[tabled(rename = "not merged")]
    ahead: String,
    path: String,
}

/// Plain text on purpose: the table library counts ANSI color codes as
/// visible characters, which throws the columns out of line.
fn state_label(state: &WorktreeState) -> String {
    match state {
        WorktreeState::Clean => "clean",
        WorktreeState::Uncommitted => "uncommitted changes",
        WorktreeState::NoWorktree => "branch only",
        WorktreeState::Unsafe(_) => "UNSAFE",
        WorktreeState::Running => "running",
        WorktreeState::Locked(_) => "locked",
    }
    .to_owned()
}

fn display_path(checkout: &Checkout, path: &Option<PathBuf>) -> String {
    match path {
        Some(path) => path
            .strip_prefix(&checkout.root)
            .unwrap_or(path)
            .display()
            .to_string(),
        None => "-".to_owned(),
    }
}

pub fn handle_worktrees_list(raw_output: bool) -> Result<()> {
    let checkout = open_checkout(&std::env::current_dir()?)?;
    let worktrees = list_agent_worktrees(&checkout)?;
    if raw_output {
        println!("{}", get_formatted_json_string(&worktrees, true)?);
        return Ok(());
    }
    if worktrees.is_empty() {
        println!("No agent worktrees in this repository.");
        return Ok(());
    }
    let rows: Vec<WorktreeRow> = worktrees
        .iter()
        .map(|worktree| WorktreeRow {
            name: worktree.name.clone(),
            status: if worktree.stale_lock {
                format!("{} (stale lock)", state_label(&worktree.state))
            } else {
                state_label(&worktree.state)
            },
            ahead: match worktree.ahead {
                0 => "none".to_owned(),
                1 => "1 commit".to_owned(),
                n => format!("{n} commits"),
            },
            path: display_path(&checkout, &worktree.path),
        })
        .collect();
    println!("{}", crate::utils::tables::build::build_table(&rows));
    println!();
    for worktree in &worktrees {
        if let WorktreeState::Unsafe(reason) = &worktree.state {
            eprintln!(
                "warning: {} is UNSAFE ({reason}); don't run git in it until you've inspected it",
                worktree.name
            );
        }
    }
    if let Some(branch) = &checkout.branch {
        println!("\"not merged\" counts commits that {branch} doesn't have yet.");
    }
    println!(
        "Continue one with `stashbase agent run --profile <profile> --resume <name> -- <agent>`."
    );
    Ok(())
}

pub fn handle_worktrees_merge(
    command: AgentWorktreesMergeCommand,
    raw_output: bool,
    silent: bool,
) -> Result<()> {
    let checkout = open_checkout(&std::env::current_dir()?)?;
    let outcome = merge_agent_worktree(
        &checkout,
        &command.name,
        command.squash,
        command.message.as_deref(),
        command.keep,
    )?;
    if raw_output {
        let json = match &outcome {
            MergeOutcome::Merged {
                commits,
                into,
                kept_running,
            } => {
                serde_json::json!({
                    "merged": true,
                    "commits": commits,
                    "into": into,
                    "kept_running_worktree": kept_running,
                })
            }
            MergeOutcome::NothingToMerge => serde_json::json!({ "merged": false, "commits": 0 }),
        };
        println!("{}", get_formatted_json_string(&json, true)?);
    } else if !silent {
        let name = command.name.trim_start_matches(BRANCH_PREFIX);
        let kept_running = matches!(
            outcome,
            MergeOutcome::Merged {
                kept_running: true,
                ..
            }
        );
        match outcome {
            MergeOutcome::Merged { commits, into, .. } => {
                let how = if command.squash {
                    "squashed into"
                } else {
                    "merged into"
                };
                println!(
                    "{} {commits} commit(s) from {name} {how} {into}.",
                    "✓".green_if_tty()
                );
            }
            MergeOutcome::NothingToMerge => {
                println!("{name} has no commits that aren't already in your branch.")
            }
        }
        if kept_running {
            println!(
                "An agent is still working in {name}, so its worktree and branch were kept; only its committed work was merged."
            );
        } else if !command.keep {
            println!("Removed the agent's worktree and branch.");
        }
    }
    Ok(())
}

pub fn handle_worktrees_clean(
    command: AgentWorktreesCleanCommand,
    raw_output: bool,
    silent: bool,
) -> Result<()> {
    let checkout = open_checkout(&std::env::current_dir()?)?;
    // Drop records of worktree dirs deleted by hand before listing.
    run_git(&checkout.root, &["worktree", "prune"])?;
    let candidates = clean_candidates(list_agent_worktrees(&checkout)?, command.all);
    if candidates.is_empty() {
        if raw_output {
            println!(
                "{}",
                get_formatted_json_string(&serde_json::json!({ "removed": [] }), true)?
            );
        } else if !silent {
            println!(
                "Nothing to clean.{}",
                if command.all {
                    ""
                } else {
                    " Unmerged agent work is kept; pass --all to remove it too."
                }
            );
        }
        return Ok(());
    }
    if !silent && !raw_output {
        println!("Will remove these agent worktrees and branches:\n");
        for worktree in &candidates {
            let detail = match (&worktree.state, worktree.ahead) {
                (WorktreeState::Unsafe(_), _) => "UNSAFE, will be skipped".to_owned(),
                (WorktreeState::Uncommitted, _) => "uncommitted changes will be lost".to_owned(),
                (_, 0) => "merged".to_owned(),
                (_, n) => format!("{n} unmerged commit(s) will be lost"),
            };
            println!("  {} ({detail})", worktree.name);
        }
        println!();
    }
    let confirmed = if command.yes {
        true
    } else if silent || raw_output {
        bail!(
            "{} agent worktree(s) to remove; re-run with --yes to remove them",
            candidates.len()
        );
    } else {
        crate::utils::interaction::confirm_opt("Remove them?").unwrap_or(false)
    };
    if !confirmed {
        return Ok(());
    }

    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for worktree in &candidates {
        match remove_agent_worktree(&checkout, worktree, command.all) {
            Ok(()) => removed.push(worktree.name.clone()),
            Err(error) => failed.push((worktree.name.clone(), error.to_string())),
        }
    }
    if raw_output {
        let failed: Vec<_> = failed
            .iter()
            .map(|(name, error)| serde_json::json!({ "name": name, "error": error }))
            .collect();
        println!(
            "{}",
            get_formatted_json_string(
                &serde_json::json!({ "removed": removed, "failed": failed }),
                true
            )?
        );
    } else {
        for (name, error) in &failed {
            eprintln!("warning: {name}: {error}");
        }
        if !silent {
            println!("Removed {} agent worktree(s).", removed.len());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::run::worktree::{
        create_run_worktree,
        test_support::{fixture, git},
    };

    fn checkout(repo: &Path) -> Checkout {
        open_checkout(repo).unwrap()
    }

    /// A worktree whose run has ended (lock released), like one kept for
    /// having uncommitted changes.
    fn finished_run(repo: &Path, name: &str) -> crate::handlers::run::worktree::RunWorktree {
        let wt = create_run_worktree(repo, name, None).unwrap();
        git(repo, &["worktree", "unlock", &wt.path.to_string_lossy()]);
        wt
    }

    /// An agent worktree at the default in-repo location with `commits`
    /// commits on its branch, whose run has ended.
    fn agent(repo: &Path, name: &str, commits: usize) -> PathBuf {
        let wt = finished_run(repo, name);
        for i in 0..commits {
            std::fs::write(wt.path.join(format!("{name}-{i}.txt")), "work").unwrap();
            git(&wt.path, &["add", "."]);
            git(&wt.path, &["commit", "-qm", &format!("{name} commit {i}")]);
        }
        wt.path
    }

    #[test]
    fn lists_agent_branches_with_state_and_unmerged_commits() {
        let (repo, _) = fixture();
        agent(&repo, "amber-river-storm", 2);
        let dirty = agent(&repo, "cedar-moss-wave", 0);
        std::fs::write(dirty.join("wip"), "wip").unwrap();
        let gone = agent(&repo, "delta-dune-echo", 1);
        git(&repo, &["worktree", "remove", &gone.to_string_lossy()]);

        let list = list_agent_worktrees(&checkout(&repo)).unwrap();
        let get = |name: &str| list.iter().find(|w| w.name == name).unwrap();
        assert_eq!(list.len(), 3, "only stashbase/* branches: {list:?}");
        assert_eq!(get("amber-river-storm").state, WorktreeState::Clean);
        assert_eq!(get("amber-river-storm").ahead, 2);
        assert_eq!(get("cedar-moss-wave").state, WorktreeState::Uncommitted);
        assert_eq!(get("delta-dune-echo").state, WorktreeState::NoWorktree);
        assert_eq!(get("delta-dune-echo").ahead, 1);
    }

    #[test]
    fn tampered_leftover_worktree_is_unsafe_and_never_touched() {
        let (repo, _) = fixture();
        let path = agent(&repo, "opal-reef-sky", 1);
        std::fs::write(path.join(".git"), "gitdir: /tmp/evil\n").unwrap();
        let checkout = checkout(&repo);
        let list = list_agent_worktrees(&checkout).unwrap();
        assert!(matches!(list[0].state, WorktreeState::Unsafe(_)));

        let error =
            merge_agent_worktree(&checkout, "opal-reef-sky", false, None, false).unwrap_err();
        assert!(error.to_string().contains("refusing to merge"), "{error}");
        assert!(remove_agent_worktree(&checkout, &list[0], true).is_err());
        assert!(
            path.exists(),
            "unsafe worktree left for the user to inspect"
        );
    }

    #[test]
    fn merge_brings_agent_commits_into_the_current_branch_and_cleans_up() {
        let (repo, _) = fixture();
        let path = agent(&repo, "amber-river-storm", 2);
        let checkout = checkout(&repo);
        let outcome =
            merge_agent_worktree(&checkout, "amber-river-storm", false, None, false).unwrap();
        assert_eq!(
            outcome,
            MergeOutcome::Merged {
                commits: 2,
                into: "main".to_owned(),
                kept_running: false
            }
        );
        assert!(repo.join("amber-river-storm-1.txt").exists());
        assert!(!path.exists(), "worktree removed");
        assert!(
            git(&repo, &["branch", "--list", "stashbase/*"]).is_empty(),
            "branch removed"
        );
        assert_eq!(
            git(&repo, &["log", "-1", "--format=%s"]),
            "Merge branch 'stashbase/amber-river-storm'"
        );
    }

    #[test]
    fn squash_merge_makes_one_commit_listing_the_agent_commits() {
        let (repo, _) = fixture();
        agent(&repo, "cedar-moss-wave", 2);
        let checkout = checkout(&repo);
        merge_agent_worktree(&checkout, "stashbase/cedar-moss-wave", true, None, false).unwrap();
        let message = git(&repo, &["log", "-1", "--format=%B"]);
        assert!(
            message.starts_with("Squash branch 'stashbase/cedar-moss-wave'\n\n"),
            "{message}"
        );
        assert!(message.contains("- cedar-moss-wave commit 0"));
        assert_eq!(
            git(&repo, &["rev-list", "--count", "--merges", "HEAD"]),
            "0"
        );
        assert!(git(&repo, &["branch", "--list", "stashbase/*"]).is_empty());
    }

    #[test]
    fn default_merge_message_is_gits_own_including_the_target_branch() {
        let (repo, _) = fixture();
        agent(&repo, "opal-reef-sky", 1);
        git(&repo, &["switch", "-q", "-c", "feature"]);
        merge_agent_worktree(&checkout(&repo), "opal-reef-sky", false, None, false).unwrap();
        assert_eq!(
            git(&repo, &["log", "-1", "--format=%s"]),
            "Merge branch 'stashbase/opal-reef-sky' into feature"
        );
    }

    #[test]
    fn custom_message_is_used_as_given_for_merge_and_squash() {
        let (repo, _) = fixture();
        agent(&repo, "amber-river-storm", 1);
        agent(&repo, "cedar-moss-wave", 2);
        let checkout = checkout(&repo);

        merge_agent_worktree(
            &checkout,
            "amber-river-storm",
            false,
            Some("Add a (agent)"),
            false,
        )
        .unwrap();
        assert_eq!(git(&repo, &["log", "-1", "--format=%B"]), "Add a (agent)");

        merge_agent_worktree(
            &checkout,
            "cedar-moss-wave",
            true,
            Some("feat: agent work\n\nBody"),
            false,
        )
        .unwrap();
        assert_eq!(
            git(&repo, &["log", "-1", "--format=%B"]),
            "feat: agent work\n\nBody"
        );

        agent(&repo, "delta-dune-echo", 1);
        let error = merge_agent_worktree(&checkout, "delta-dune-echo", false, Some("  "), false)
            .unwrap_err();
        assert!(error.to_string().contains("must not be empty"), "{error}");
    }

    #[test]
    fn merge_refuses_uncommitted_work_on_either_side() {
        let (repo, _) = fixture();
        let path = agent(&repo, "iris-jade-lotus", 1);
        let checkout = checkout(&repo);

        std::fs::write(path.join("wip"), "wip").unwrap();
        let error =
            merge_agent_worktree(&checkout, "iris-jade-lotus", false, None, false).unwrap_err();
        assert!(error.to_string().contains("uncommitted changes"), "{error}");
        std::fs::remove_file(path.join("wip")).unwrap();

        std::fs::write(repo.join("sub/f"), "user edit").unwrap();
        let error =
            merge_agent_worktree(&checkout, "iris-jade-lotus", false, None, false).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("your checkout has uncommitted changes"),
            "{error}"
        );
        assert!(path.exists() && git(&repo, &["branch", "--list", "stashbase/*"]).contains("iris"));
    }

    #[test]
    fn merge_conflict_keeps_the_agent_worktree_and_branch() {
        let (repo, _) = fixture();
        let wt = finished_run(&repo, "lava-conflict");
        std::fs::write(wt.path.join("sub/f"), "agent").unwrap();
        git(&wt.path, &["commit", "-qam", "agent edit"]);
        std::fs::write(repo.join("sub/f"), "user").unwrap();
        git(&repo, &["commit", "-qam", "user edit"]);

        let error = merge_agent_worktree(&checkout(&repo), "lava-conflict", false, None, false)
            .unwrap_err();
        assert!(error.to_string().contains("git merge --abort"), "{error}");
        assert!(wt.path.exists());
        git(&repo, &["merge", "--abort"]);
    }

    #[test]
    fn clean_removes_only_merged_clean_work_unless_all() {
        let (repo, _) = fixture();
        agent(&repo, "merged-one", 0);
        agent(&repo, "unmerged-one", 1);
        let checkout = checkout(&repo);

        let default = clean_candidates(list_agent_worktrees(&checkout).unwrap(), false);
        assert_eq!(
            default.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
            vec!["merged-one"]
        );
        for worktree in &default {
            remove_agent_worktree(&checkout, worktree, false).unwrap();
        }
        let all = clean_candidates(list_agent_worktrees(&checkout).unwrap(), true);
        assert_eq!(all.len(), 1);
        remove_agent_worktree(&checkout, &all[0], true).unwrap();
        assert!(list_agent_worktrees(&checkout).unwrap().is_empty());
    }

    #[test]
    fn running_worktree_is_merged_but_never_removed() {
        let (repo, _) = fixture();
        // Still locked by this (live) test process: a run in progress.
        let wt = create_run_worktree(&repo, "live-run", None).unwrap();
        std::fs::write(wt.path.join("done.txt"), "x").unwrap();
        git(&wt.path, &["add", "."]);
        git(&wt.path, &["commit", "-qm", "partial work"]);
        std::fs::write(wt.path.join("in-progress.txt"), "x").unwrap();
        let checkout = checkout(&repo);

        let list = list_agent_worktrees(&checkout).unwrap();
        assert_eq!(list[0].state, WorktreeState::Running);
        assert!(
            clean_candidates(list.clone(), true).is_empty(),
            "never cleaned, even with --all"
        );
        assert!(remove_agent_worktree(&checkout, &list[0], true).is_err());

        let outcome = merge_agent_worktree(&checkout, "live-run", false, None, false).unwrap();
        assert!(matches!(
            outcome,
            MergeOutcome::Merged {
                kept_running: true,
                commits: 1,
                ..
            }
        ));
        assert!(repo.join("done.txt").exists(), "committed work merged");
        assert!(
            wt.path.join("in-progress.txt").exists(),
            "worktree untouched"
        );
    }

    #[test]
    fn stale_lock_from_a_killed_run_is_released_on_clean() {
        let (repo, _) = fixture();
        let wt = finished_run(&repo, "killed-run");
        let path = wt.path.to_string_lossy().into_owned();
        let reason = format!(
            "{}; pid=999999; started=Thu Jan  1 00:00:00 1970",
            crate::handlers::run::worktree::LOCK_REASON_PREFIX
        );
        git(&repo, &["worktree", "lock", "--reason", &reason, &path]);
        let checkout = checkout(&repo);

        let list = list_agent_worktrees(&checkout).unwrap();
        assert_eq!(list[0].state, WorktreeState::Clean);
        assert!(list[0].stale_lock);
        let candidates = clean_candidates(list, false);
        assert_eq!(candidates.len(), 1);
        remove_agent_worktree(&checkout, &candidates[0], false).unwrap();
        assert!(!wt.path.exists());
    }

    #[test]
    fn worktree_locked_by_the_user_is_left_alone() {
        let (repo, _) = fixture();
        let wt = finished_run(&repo, "usb-drive");
        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "on a USB drive",
                &wt.path.to_string_lossy(),
            ],
        );
        let checkout = checkout(&repo);
        let list = list_agent_worktrees(&checkout).unwrap();
        assert_eq!(
            list[0].state,
            WorktreeState::Locked("on a USB drive".to_owned())
        );
        assert!(clean_candidates(list, true).is_empty());
    }

    #[test]
    fn refuses_to_run_from_inside_an_agent_worktree() {
        let (repo, _) = fixture();
        let path = agent(&repo, "sage-sand-sky", 0);
        let error = open_checkout(&path).unwrap_err();
        assert!(error
            .to_string()
            .contains("run `stashbase agent worktrees` from your own checkout"));
    }
}
