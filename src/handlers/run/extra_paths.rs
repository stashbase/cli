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

/// Resolves entries relative to `cwd` (the directory the run started in,
/// never the worktree) and refuses anything too broad or sensitive to hand
/// the agent: `/`, `$HOME` or an ancestor of it, credential directories,
/// the working directory itself or anything overlapping it, and duplicates.
pub fn resolve_extra_paths(
    entries: &[WorkspaceExtraPath],
    cwd: &Path,
    home: Option<&Path>,
) -> Result<Vec<ResolvedExtraPath>, String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", cwd.display()))?;
    let home = home.and_then(|home| home.canonicalize().ok());
    let mut resolved: Vec<ResolvedExtraPath> = Vec::new();
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
        if path.starts_with(&cwd) || cwd.starts_with(&path) {
            return Err(format!(
                "workspace extra path '{raw}' overlaps the working directory {}",
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
    Ok(resolved)
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

/// Docker backend: a linked worktree's `.git` is a file pointing into its
/// main repository, which the container doesn't see, so git breaks there.
pub fn docker_git_worktree_notes(extra_paths: &[ResolvedExtraPath]) -> Vec<String> {
    extra_paths
        .iter()
        .filter(|extra| extra.path.join(".git").is_file())
        .map(|extra| {
            format!(
                "Note: {} is a git worktree; git commands inside the sandbox won't work there because its main repository isn't mounted.",
                extra.path.display()
            )
        })
        .collect()
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
            (".", "working directory"),
            ("sub", "working directory"),
            ("..", "working directory"),
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
    fn worktree_note_only_when_a_worktree_has_extra_paths() {
        let extra = [resolved(PathBuf::from("/srv/backend"), false)];
        assert!(worktree_note(true, &extra).is_some());
        assert!(worktree_note(false, &extra).is_none());
        assert!(worktree_note(true, &[]).is_none());
    }

    #[test]
    fn notes_extra_paths_that_are_linked_git_worktrees() {
        let fixture = Fixture::new("git-worktree");
        let backend = fixture.home().join("code/backend");
        let docs = fixture.home().join("code/docs");
        std::fs::write(
            backend.join(".git"),
            "gitdir: /elsewhere/.git/worktrees/x\n",
        )
        .unwrap();
        std::fs::create_dir_all(docs.join(".git")).unwrap();
        let notes =
            docker_git_worktree_notes(&[resolved(backend.clone(), false), resolved(docs, false)]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].contains(&backend.display().to_string()),
            "{}",
            notes[0]
        );
    }
}
