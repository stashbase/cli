//! Confines the host-side secret scan the Agent Proxy runs for a sandboxed
//! agent. The scan reads a repository the agent controls, so a symlink, a
//! `gitdir:` file, `objects/info/alternates` or `include.path` could
//! otherwise point it at any file on the host. Under this confinement the
//! scan sees roughly what the agent sees: home directories are hidden except
//! the run's working tree, its git directories and the CLI binary, and the
//! profile's `deny_read` entries stay denied.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct ScanConfinement {
    workdir: PathBuf,
    git_dirs: Vec<PathBuf>,
    exe: PathBuf,
    denied_read: Vec<String>,
}

impl ScanConfinement {
    /// Captures the run's git directories now, before the agent can point
    /// `.git` anywhere else.
    pub fn for_run(workdir: &Path, exe: &Path, denied_read: &[String]) -> Self {
        let workdir = canonical(workdir);
        let mut git_dirs = Vec::new();
        if let Ok(repo) = git2::Repository::discover(&workdir) {
            for dir in [repo.path(), repo.commondir()] {
                let dir = canonical(dir);
                if !dir.starts_with(&workdir) && !git_dirs.contains(&dir) {
                    git_dirs.push(dir);
                }
            }
        }
        Self {
            workdir,
            git_dirs,
            exe: canonical(exe),
            denied_read: denied_read.to_vec(),
        }
    }

    /// The program and arguments that run `exe args` confined, with `home`
    /// (an empty, writable directory) standing in for the user's home.
    pub fn wrap(&self, args: &[&str], home: &Path) -> Result<(String, Vec<String>)> {
        platform::wrap(self, args, home)
    }

    #[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
    fn readable(&self, home: &Path) -> Vec<PathBuf> {
        let mut paths = vec![self.workdir.clone(), self.exe.clone(), canonical(home)];
        paths.extend(self.git_dirs.iter().cloned());
        paths
    }
}

/// Why secret scans can't be confined on this machine, if they can't.
pub fn unavailable_reason() -> Option<String> {
    platform::unavailable_reason()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

/// Roots that hold users' files, hidden from the scan.
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
fn hidden_roots(candidates: &[&str]) -> Vec<PathBuf> {
    let mut roots = candidates
        .iter()
        .map(PathBuf::from)
        .chain(std::env::var_os("HOME").map(PathBuf::from))
        .filter(|path| path.exists())
        .map(|path| canonical(&path))
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    // A root inside another root adds nothing.
    let all = roots.clone();
    roots.retain(|root| {
        !all.iter()
            .any(|other| other != root && root.starts_with(other))
    });
    roots
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{hidden_roots, ScanConfinement};
    use crate::handlers::run::subprocess::{denied_file_rules, escape_sbpl_path};
    use anyhow::Result;
    use std::path::{Path, PathBuf};

    pub fn unavailable_reason() -> Option<String> {
        None
    }

    pub fn wrap(
        confinement: &ScanConfinement,
        args: &[&str],
        home: &Path,
    ) -> Result<(String, Vec<String>)> {
        let mut command = vec!["-p".to_owned(), profile(confinement, home)?];
        command.push(confinement.exe.to_string_lossy().into_owned());
        command.extend(args.iter().map(|arg| (*arg).to_owned()));
        Ok(("/usr/bin/sandbox-exec".to_owned(), command))
    }

    pub(super) fn profile(confinement: &ScanConfinement, home: &Path) -> Result<String> {
        let readable = confinement.readable(home);
        let quote = |path: &Path| format!("\"{}\"", escape_sbpl_path(&path.to_string_lossy()));

        let mut rules = vec!["(version 1)".to_owned(), "(allow default)".to_owned()];
        for root in hidden_roots(&["/Users", "/Volumes", "/private/var/root"]) {
            rules.push(format!("(deny file-read* (subpath {}))", quote(&root)));
        }
        // Path resolution (realpath, libgit2's repository lookup) stats each
        // ancestor of the paths the scan needs; that metadata is all it gets.
        let mut ancestors = readable
            .iter()
            .flat_map(|path| path.ancestors().skip(1).map(PathBuf::from))
            .collect::<Vec<_>>();
        ancestors.sort();
        ancestors.dedup();
        for ancestor in ancestors {
            rules.push(format!(
                "(allow file-read-metadata (literal {}))",
                quote(&ancestor)
            ));
        }
        for path in &readable {
            let filter = if path.is_file() { "literal" } else { "subpath" };
            rules.push(format!("(allow file-read* ({filter} {}))", quote(path)));
        }
        // Last, so the profile's own denials win inside the working tree.
        rules.push(denied_file_rules(
            &confinement.denied_read,
            &[],
            &confinement.workdir,
        )?);
        Ok(rules.join("\n"))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{hidden_roots, ScanConfinement};
    use crate::handlers::run::{docker_sandbox::empty_shadow_file_path, fs_rules};
    use anyhow::Result;
    use std::path::{Path, PathBuf};

    fn executable() -> Option<&'static str> {
        ["bwrap", "bubblewrap"].into_iter().find(|name| {
            std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path).any(|directory| directory.join(name).is_file())
            })
        })
    }

    pub fn unavailable_reason() -> Option<String> {
        let Some(executable) = executable() else {
            return Some(
                "secret_scan needs bubblewrap (`bwrap`) on Linux to confine the host-side scan"
                    .to_owned(),
            );
        };
        let probe = std::process::Command::new(executable)
            .args([
                "--die-with-parent",
                "--ro-bind",
                "/",
                "/",
                "--tmpfs",
                "/tmp",
                "--",
                "/bin/true",
            ])
            .output();
        match probe {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(format!(
                "secret_scan needs a working bubblewrap to confine the host-side scan: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => Some(format!(
                "secret_scan needs a working bubblewrap to confine the host-side scan: {error}"
            )),
        }
    }

    pub fn wrap(
        confinement: &ScanConfinement,
        args: &[&str],
        home: &Path,
    ) -> Result<(String, Vec<String>)> {
        let executable = executable()
            .ok_or_else(|| anyhow::anyhow!("bubblewrap is not available"))?
            .to_owned();
        let mut command = bwrap_args(confinement, home).map_err(anyhow::Error::msg)?;
        command.push("--".to_owned());
        command.push(confinement.exe.to_string_lossy().into_owned());
        command.extend(args.iter().map(|arg| (*arg).to_owned()));
        Ok((executable, command))
    }

    pub(super) fn bwrap_args(
        confinement: &ScanConfinement,
        home: &Path,
    ) -> Result<Vec<String>, String> {
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let mut args = vec![
            "--die-with-parent".to_owned(),
            "--ro-bind".to_owned(),
            "/".to_owned(),
            "/".to_owned(),
            "--proc".to_owned(),
            "/proc".to_owned(),
            "--dev".to_owned(),
            "/dev".to_owned(),
        ];
        for root in hidden_roots(&["/home", "/root", "/mnt", "/media", "/srv"]) {
            args.extend(["--tmpfs".to_owned(), text(&root)]);
        }
        let home = super::canonical(home);
        for path in confinement.readable(&home) {
            let mode = if path == home { "--bind" } else { "--ro-bind" };
            args.extend([mode.to_owned(), text(&path), text(&path)]);
        }
        for path in
            fs_rules::expand_policy_entries(&confinement.denied_read, &confinement.workdir, None)?
        {
            if PathBuf::from(&path).is_dir() {
                args.extend(["--tmpfs".to_owned(), path]);
            } else {
                args.extend(["--ro-bind".to_owned(), empty_shadow_file_path()?, path]);
            }
        }
        args.extend(["--chdir".to_owned(), text(&confinement.workdir)]);
        Ok(args)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::ScanConfinement;
    use anyhow::Result;
    use std::path::Path;

    pub fn unavailable_reason() -> Option<String> {
        Some("secret_scan is supported only on macOS and Linux, where the host-side scan can be confined".to_owned())
    }

    pub fn wrap(_: &ScanConfinement, _: &[&str], _: &Path) -> Result<(String, Vec<String>)> {
        anyhow::bail!(unavailable_reason().unwrap_or_default())
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use std::fs;

    struct Layout {
        root: PathBuf,
        workdir: PathBuf,
        home: PathBuf,
        outside: PathBuf,
    }

    /// A repo, a scratch home, and a secret outside both, all inside a
    /// directory under the real home so the hidden roots cover it.
    fn layout() -> Option<Layout> {
        let base = std::env::var_os("HOME").map(PathBuf::from)?;
        let root = base.join(format!(
            ".stashbase-scan-sandbox-test-{}",
            uuid::Uuid::new_v4()
        ));
        let workdir = root.join("repo");
        let home =
            std::env::temp_dir().join(format!("stashbase-scan-home-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&workdir).ok()?;
        fs::create_dir_all(&home).ok()?;
        git2::Repository::init(&workdir).ok()?;
        fs::write(workdir.join("inside.txt"), "inside").ok()?;
        let outside = root.join("outside-secret.txt");
        fs::write(&outside, "outside").ok()?;
        std::os::unix::fs::symlink(&outside, workdir.join("link.txt")).ok()?;
        Some(Layout {
            root: canonical(&root),
            workdir: canonical(&workdir),
            home,
            outside: canonical(&outside),
        })
    }

    fn run_cat(confinement: &ScanConfinement, home: &Path, file: &Path) -> std::process::Output {
        let (program, args) = confinement.wrap(&[&file.to_string_lossy()], home).unwrap();
        std::process::Command::new(program)
            .args(args)
            .current_dir(&confinement.workdir)
            .output()
            .unwrap()
    }

    #[test]
    fn confined_scan_reads_the_worktree_but_not_host_files_it_links_to() {
        if let Some(reason) = unavailable_reason() {
            eprintln!("skipping: {reason}");
            return;
        }
        let Some(layout) = layout() else {
            eprintln!("skipping: no writable HOME");
            return;
        };
        // `cat` stands in for the CLI binary: the confinement is about which
        // files the process can read, not what it does with them.
        let cat = canonical(Path::new("/bin/cat"));
        let confinement = ScanConfinement::for_run(&layout.workdir, &cat, &[]);

        let inside = run_cat(
            &confinement,
            &layout.home,
            &layout.workdir.join("inside.txt"),
        );
        let via_link = run_cat(&confinement, &layout.home, &layout.workdir.join("link.txt"));
        let direct = run_cat(&confinement, &layout.home, &layout.outside);

        assert_eq!(String::from_utf8_lossy(&inside.stdout), "inside");
        assert!(
            !via_link.status.success(),
            "followed a symlink out of the worktree"
        );
        assert!(
            !direct.status.success(),
            "read a host file outside the worktree"
        );
        let _ = fs::remove_dir_all(&layout.root);
        let _ = fs::remove_dir_all(&layout.home);
    }

    #[test]
    fn confined_scan_keeps_profile_deny_read_inside_the_worktree() {
        if let Some(reason) = unavailable_reason() {
            eprintln!("skipping: {reason}");
            return;
        }
        let Some(layout) = layout() else {
            eprintln!("skipping: no writable HOME");
            return;
        };
        fs::write(layout.workdir.join(".env"), "SECRET=1").unwrap();
        let cat = canonical(Path::new("/bin/cat"));
        let confinement = ScanConfinement::for_run(&layout.workdir, &cat, &[".env".to_owned()]);

        let denied = run_cat(&confinement, &layout.home, &layout.workdir.join(".env"));

        assert!(
            !String::from_utf8_lossy(&denied.stdout).contains("SECRET"),
            "read a deny_read file"
        );
        let _ = fs::remove_dir_all(&layout.root);
        let _ = fs::remove_dir_all(&layout.home);
    }

    #[test]
    fn for_run_captures_git_dirs_outside_the_worktree() {
        let Some(layout) = layout() else {
            return;
        };
        let cat = canonical(Path::new("/bin/cat"));

        let confinement = ScanConfinement::for_run(&layout.workdir, &cat, &[]);

        // A plain repo's `.git` is inside the worktree, so nothing extra.
        assert!(confinement.git_dirs.is_empty());
        assert_eq!(confinement.workdir, layout.workdir);
        let _ = fs::remove_dir_all(&layout.root);
        let _ = fs::remove_dir_all(&layout.home);
    }
}
