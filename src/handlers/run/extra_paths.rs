//! `[workspace] extra_paths`: other local directories an agent run works on
//! besides the current repo (e.g. a sibling backend checkout).
//!
//! The Docker backend mounts each resolved path at its own host path (see
//! `docker_sandbox::append_extra_path_mounts`). The native backend's
//! filesystem rules are deny lists, so it already sees them; a read-only
//! entry just becomes a deny-write path there.

use std::path::{Path, PathBuf};

use crate::models::agent::WorkspaceExtraPath;

/// A validated extra path: absolute, canonical, an existing directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExtraPath {
    pub path: PathBuf,
    pub read_only: bool,
}

/// Directories under `$HOME` that hold credentials; never mountable, nor
/// anything containing or inside them.
const CREDENTIAL_DIRS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".docker",
    ".kube",
    ".config/stashbase",
    ".config/gcloud",
];

/// The outcome of `resolve_extra_paths`: what to mount, and the entries
/// (as written) skipped because the working directory already covers them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtraPathResolution {
    pub paths: Vec<ResolvedExtraPath>,
    pub skipped: Vec<String>,
}

impl ExtraPathResolution {
    /// One line for `agent validate`: what is mounted and what is skipped.
    pub fn summary(&self) -> String {
        let mounted = if self.paths.is_empty() {
            "none mounted".to_owned()
        } else {
            self.paths
                .iter()
                .map(|extra| {
                    format!(
                        "{}{}",
                        extra.path.display(),
                        if extra.read_only { " (read-only)" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        if self.skipped.is_empty() {
            mounted
        } else {
            format!(
                "{mounted}; skipped, already inside the working directory: {}",
                self.skipped.join(", ")
            )
        }
    }

    /// Said at run start only when every entry was skipped: the profile
    /// lists other folders, yet the run adds none, which usually means it
    /// was started from a directory containing them all (e.g. ~/Projects).
    pub fn all_skipped_note(&self) -> Option<String> {
        (self.paths.is_empty() && !self.skipped.is_empty()).then(|| {
            format!(
                "Note: every [workspace] extra path is inside the working directory, so the agent sees all of {} instead: {}",
                std::env::current_dir()
                    .map(|cwd| cwd.display().to_string())
                    .unwrap_or_else(|_| "it".to_owned()),
                self.skipped.join(", ")
            )
        })
    }
}

/// Refuses an agent run whose working directory hands the agent far more
/// than a project: `/`, `$HOME` or an ancestor of it, or a credential
/// directory. Both backends give the agent the working directory, so this
/// applies whatever the profile says — and with global profiles an agent is
/// easy to start from anywhere.
pub fn ensure_working_directory_is_not_too_broad(
    cwd: &Path,
    home: Option<&Path>,
) -> Result<(), String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", cwd.display()))?;
    let hint = "run the agent from a project directory, and list other folders under [workspace] extra_paths";
    if cwd.parent().is_none() {
        return Err(format!(
            "the working directory is the filesystem root; {hint}"
        ));
    }
    let Some(home) = home.and_then(|home| home.canonicalize().ok()) else {
        return Ok(());
    };
    if home.starts_with(&cwd) {
        return Err(format!(
            "the working directory {} would expose your whole home directory; {hint}",
            cwd.display()
        ));
    }
    for dir in CREDENTIAL_DIRS {
        let credentials = home.join(dir);
        if cwd.starts_with(&credentials) {
            return Err(format!(
                "the working directory is inside the credential directory {}; {hint}",
                credentials.display()
            ));
        }
    }
    Ok(())
}

/// Resolves entries relative to `cwd` (the directory the run started in,
/// never the worktree) and refuses anything too broad or sensitive to hand
/// the agent: `/`, `$HOME` or an ancestor of it, credential directories,
/// a parent of the working directory, and overlapping entries. An entry
/// that is the working directory, or inside it, is skipped: it's already
/// visible.
pub fn resolve_extra_paths(
    entries: &[WorkspaceExtraPath],
    cwd: &Path,
    home: Option<&Path>,
) -> Result<ExtraPathResolution, String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", cwd.display()))?;
    let home = home.and_then(|home| home.canonicalize().ok());
    let mut resolved: Vec<ResolvedExtraPath> = Vec::new();
    let mut skipped = Vec::new();
    for entry in entries {
        let raw = entry.path();
        if raw.trim().is_empty() {
            return Err("workspace extra path must not be empty".to_owned());
        }
        let expanded = match (raw.strip_prefix("~/"), &home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(raw),
        };
        let path = cwd
            .join(expanded)
            .canonicalize()
            .map_err(|error| format!("workspace extra path '{raw}' cannot be resolved: {error}"))?;
        if !path.is_dir() {
            return Err(format!("workspace extra path '{raw}' is not a directory"));
        }
        if path.parent().is_none() {
            return Err(format!(
                "workspace extra path '{raw}' is the filesystem root"
            ));
        }
        if let Some(home) = &home {
            if home.starts_with(&path) {
                return Err(format!(
                    "workspace extra path '{raw}' would expose your whole home directory"
                ));
            }
            for dir in CREDENTIAL_DIRS {
                let credentials = home.join(dir);
                if path.starts_with(&credentials) || credentials.starts_with(&path) {
                    return Err(format!(
                        "workspace extra path '{raw}' overlaps the credential directory {}",
                        credentials.display()
                    ));
                }
            }
        }
        if path.starts_with(&cwd) {
            // Already visible as the working directory itself, so one
            // global profile can list every repo of a set and run from any.
            // Only a read-only entry nested inside would be lost that way.
            if entry.read_only() && path != cwd {
                return Err(format!(
                    "workspace extra path '{raw}' is inside the working directory {}; use [filesystem] deny_write to make it read-only",
                    cwd.display()
                ));
            }
            skipped.push(raw.to_owned());
            continue;
        }
        if cwd.starts_with(&path) {
            return Err(format!(
                "workspace extra path '{raw}' contains the working directory {}; list the repositories you need instead",
                cwd.display()
            ));
        }
        if let Some(other) = resolved
            .iter()
            .find(|other| other.path.starts_with(&path) || path.starts_with(&other.path))
        {
            return Err(format!(
                "workspace extra path '{raw}' overlaps {}, which is already listed",
                other.path.display()
            ));
        }
        resolved.push(ResolvedExtraPath {
            path,
            read_only: entry.read_only(),
        });
    }
    Ok(ExtraPathResolution {
        paths: resolved,
        skipped,
    })
}

/// `deny_write` entries that keep read-only extra paths read-only on the
/// native backend (its policy is a deny-list, so nothing else is needed).
pub fn read_only_deny_entries(extra_paths: &[ResolvedExtraPath]) -> Vec<String> {
    extra_paths
        .iter()
        .filter(|extra| extra.read_only)
        .map(|extra| super::fs_rules::literal_entry(&extra.path.to_string_lossy()))
        .collect()
}

/// A worktree only covers the current repo; say so when extra paths are
/// edited in place next to it.
pub fn worktree_note(worktree: bool, extra_paths: &[ResolvedExtraPath]) -> Option<String> {
    (worktree && !extra_paths.is_empty()).then(|| {
        "Note: the worktree covers only this repo; edits in [workspace] extra_paths land directly in those checkouts.".to_owned()
    })
}

/// Docker backend: git dirs the agent can write to (writable extra repos'
/// `.git`, linked worktrees' common dirs, the run's own common dir) whose
/// `commondir` it must not plant. Git reads that file from any git dir and
/// takes the config — `core.fsmonitor`, `core.hooksPath` — of whatever dir
/// it names, so a planted one would make host git run agent code. A
/// read-only mount can't cover a file that doesn't exist yet (Docker would
/// create it on the host), so this snapshots which dirs have none before
/// the run and removes any that appear by the end.
pub struct GitDirWatch {
    without_commondir: Vec<PathBuf>,
}

impl GitDirWatch {
    pub fn new(git_dirs: Vec<PathBuf>) -> Self {
        let without_commondir = git_dirs
            .into_iter()
            .filter(|dir| std::fs::symlink_metadata(dir.join("commondir")).is_err())
            .collect();
        Self { without_commondir }
    }

    /// Removes each `commondir` created during the run; returns a warning
    /// per removal (or failure to remove).
    pub fn remove_new_commondirs(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for dir in &self.without_commondir {
            let commondir = dir.join("commondir");
            let Ok(meta) = std::fs::symlink_metadata(&commondir) else {
                continue;
            };
            let removed = if meta.is_dir() {
                std::fs::remove_dir_all(&commondir)
            } else {
                std::fs::remove_file(&commondir)
            };
            warnings.push(match removed {
                Ok(()) => format!(
                    "Warning: removed {} created during the run; it would have redirected git to another repository's config.",
                    commondir.display()
                ),
                Err(error) => format!(
                    "Warning: {} was created during the run and could not be removed ({error}); delete it before running git there.",
                    commondir.display()
                ),
            });
        }
        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::agent::WorkspaceExtraPathEntry;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "stashbase-extra-paths-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            for dir in [
                "home/.ssh",
                "home/code/cli/sub",
                "home/code/backend",
                "home/code/docs",
            ] {
                std::fs::create_dir_all(root.join(dir)).unwrap();
            }
            std::fs::write(root.join("home/code/file.txt"), "").unwrap();
            Fixture { root }
        }

        fn home(&self) -> PathBuf {
            self.root.join("home")
        }

        fn cwd(&self) -> PathBuf {
            self.root.join("home/code/cli")
        }

        fn resolve(
            &self,
            entries: &[WorkspaceExtraPath],
        ) -> Result<Vec<ResolvedExtraPath>, String> {
            self.resolution(entries).map(|resolution| resolution.paths)
        }

        fn resolution(
            &self,
            entries: &[WorkspaceExtraPath],
        ) -> Result<ExtraPathResolution, String> {
            resolve_extra_paths(entries, &self.cwd(), Some(&self.home()))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn bare(path: &str) -> WorkspaceExtraPath {
        WorkspaceExtraPath::Path(path.to_owned())
    }

    #[test]
    fn resolves_relative_entries_against_the_working_directory() {
        let fixture = Fixture::new("relative");
        let resolved = fixture
            .resolve(&[
                bare("../backend"),
                WorkspaceExtraPath::Detailed(WorkspaceExtraPathEntry {
                    path: "~/code/docs".to_owned(),
                    read_only: true,
                }),
            ])
            .unwrap();
        let code = fixture.home().join("code").canonicalize().unwrap();
        assert_eq!(
            resolved,
            vec![
                ResolvedExtraPath {
                    path: code.join("backend"),
                    read_only: false,
                },
                ResolvedExtraPath {
                    path: code.join("docs"),
                    read_only: true,
                },
            ]
        );
    }

    #[test]
    fn refuses_broad_sensitive_or_overlapping_paths() {
        let fixture = Fixture::new("refuse");
        for (entry, expected) in [
            ("/", "filesystem root"),
            ("~/", "home directory"),
            ("../..", "home directory"),
            ("~/.ssh", "credential directory"),
            ("..", "contains the working directory"),
            ("../missing", "cannot be resolved"),
            ("../file.txt", "not a directory"),
            ("", "must not be empty"),
        ] {
            let error = fixture.resolve(&[bare(entry)]).unwrap_err();
            assert!(error.contains(expected), "{entry}: {error}");
        }
        let error = fixture
            .resolve(&[bare("../backend"), bare("~/code/backend")])
            .unwrap_err();
        assert!(error.contains("already listed"), "{error}");
    }

    fn resolved(path: PathBuf, read_only: bool) -> ResolvedExtraPath {
        ResolvedExtraPath { path, read_only }
    }

    #[test]
    fn only_read_only_extra_paths_become_literal_deny_write_entries() {
        let fixture = Fixture::new("deny-write");
        let backend = fixture.home().join("code/backend");
        let docs = fixture.home().join("code/docs");
        let entries = read_only_deny_entries(&[
            resolved(backend.clone(), false),
            resolved(docs.clone(), true),
        ]);
        assert_eq!(
            entries,
            vec![super::super::fs_rules::literal_entry(
                &docs.to_string_lossy()
            )]
        );
        let rules = super::super::fs_rules::parse_fs_rules(&entries, &fixture.cwd()).unwrap();
        assert!(
            matches!(
                rules.as_slice(),
                [super::super::fs_rules::FsRule::Path(path)] if *path == docs.to_string_lossy()
            ),
            "{rules:?}"
        );
    }

    #[test]
    fn skips_entries_that_are_the_working_directory_or_inside_it() {
        let fixture = Fixture::new("skip-cwd");
        let paths = fixture
            .resolve(&[
                bare("~/code/cli"),
                bare("."),
                bare("sub"),
                WorkspaceExtraPath::Detailed(WorkspaceExtraPathEntry {
                    path: "~/code/cli".to_owned(),
                    read_only: true,
                }),
                bare("~/code/backend"),
            ])
            .unwrap();
        let code = fixture.home().join("code").canonicalize().unwrap();
        assert_eq!(paths, vec![resolved(code.join("backend"), false)]);

        let error = fixture
            .resolve(&[WorkspaceExtraPath::Detailed(WorkspaceExtraPathEntry {
                path: "sub".to_owned(),
                read_only: true,
            })])
            .unwrap_err();
        assert!(error.contains("deny_write"), "{error}");
    }

    #[test]
    fn reports_skipped_entries() {
        let fixture = Fixture::new("skipped");
        let resolution = fixture
            .resolution(&[bare("~/code/cli"), bare("../backend")])
            .unwrap();
        assert_eq!(resolution.skipped, vec!["~/code/cli".to_owned()]);
        let summary = resolution.summary();
        assert!(summary.contains("backend"), "{summary}");
        assert!(
            summary.contains("skipped, already inside the working directory: ~/code/cli"),
            "{summary}"
        );
        assert!(
            resolution.all_skipped_note().is_none(),
            "something is still mounted"
        );

        let all_skipped = fixture.resolution(&[bare("."), bare("sub")]).unwrap();
        assert!(
            all_skipped.summary().starts_with("none mounted; skipped"),
            "{}",
            all_skipped.summary()
        );
        assert!(all_skipped.all_skipped_note().is_some());
    }

    #[test]
    fn refuses_working_directories_that_expose_too_much() {
        let fixture = Fixture::new("broad-cwd");
        let home = fixture.home();
        let check = |cwd: &Path| ensure_working_directory_is_not_too_broad(cwd, Some(&home));
        assert!(check(&fixture.cwd()).is_ok());
        assert!(
            check(&home.join("code")).is_ok(),
            "a folder of projects is a deliberate choice"
        );
        for (cwd, expected) in [
            (PathBuf::from("/"), "filesystem root"),
            (home.clone(), "whole home directory"),
            (fixture.root.clone(), "whole home directory"),
            (home.join(".ssh"), "credential directory"),
        ] {
            let error = check(&cwd).unwrap_err();
            assert!(error.contains(expected), "{}: {error}", cwd.display());
            assert!(error.contains("extra_paths"), "{error}");
        }
    }

    #[test]
    fn worktree_note_only_when_a_worktree_has_extra_paths() {
        let extra = [resolved(PathBuf::from("/srv/backend"), false)];
        assert!(worktree_note(true, &extra).is_some());
        assert!(worktree_note(false, &extra).is_none());
        assert!(worktree_note(true, &[]).is_none());
    }

    #[test]
    fn git_dir_watch_removes_only_commondirs_created_during_the_run() {
        let fixture = Fixture::new("commondir");
        let fresh = fixture.home().join("code/backend/.git");
        let existing = fixture.home().join("code/docs/.git");
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("commondir"), "../main/.git\n").unwrap();

        let watch = GitDirWatch::new(vec![fresh.clone(), existing.clone()]);
        assert!(watch.remove_new_commondirs().is_empty());

        std::fs::write(fresh.join("commondir"), "/tmp/evil\n").unwrap();
        let warnings = watch.remove_new_commondirs();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("removed"), "{}", warnings[0]);
        assert!(!fresh.join("commondir").exists());
        assert!(existing.join("commondir").exists(), "pre-existing one kept");
    }
}
