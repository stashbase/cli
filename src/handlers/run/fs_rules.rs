//! Parsing and matching for `[filesystem] deny_read` / `deny_write` entries.
//!
//! An entry is one of:
//! - a plain path (`~/.ssh`, `secrets`): denies the path and everything under it;
//! - a glob, when it contains `*` or `?` (`**/.env*`): `*` and `?` stay within
//!   one path segment, `**` crosses segments, `[...]` is a character class;
//! - a regex, when prefixed `re:` (`re:^~/.*\.pem$`), matched against the
//!   absolute path and required to start with `^/` or `^~/`;
//! - `path:<path>`, which forces a plain path even if it contains `*` or `?`.
//!
//! `[` alone does not make an entry a glob: plain paths containing brackets
//! were accepted before globs existed and must keep their meaning.
//!
//! Seatbelt enforces patterns live via its `regex` filter, so the regex built
//! here sticks to the subset both it and the `regex` crate understand. Every
//! other backend only accepts concrete paths and uses `expand_fs_rules`, a
//! snapshot taken at launch.

use std::{
    env,
    path::{Path, PathBuf},
};

const REGEX_PREFIX: &str = "re:";
const LITERAL_PREFIX: &str = "path:";
/// Upper bound on directory entries visited per pattern during expansion, so a
/// broad pattern rooted at `~` or `/` can't stall agent startup.
const MAX_WALK_ENTRIES: usize = 200_000;

#[derive(Debug, Clone)]
pub(crate) enum FsRule {
    /// Absolute path; denies it and everything under it.
    Path(String),
    Pattern {
        /// The entry as configured, for diagnostics.
        source: String,
        /// Anchored regex over absolute paths, valid for Seatbelt and `regex`.
        #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
        regex: String,
        compiled: regex::Regex,
        /// Deepest directory every match must live under.
        root: PathBuf,
    },
}

impl FsRule {
    pub(crate) fn matches(&self, path: &Path) -> bool {
        match self {
            Self::Path(denied) => path.starts_with(denied),
            Self::Pattern { compiled, .. } => compiled.is_match(&path.to_string_lossy()),
        }
    }
}

pub(crate) fn is_pattern_entry(entry: &str) -> bool {
    let entry = entry.trim();
    entry.starts_with(REGEX_PREFIX)
        || (!entry.starts_with(LITERAL_PREFIX) && entry.contains(['*', '?']))
}

/// Wraps a generated path so it is never parsed as a pattern.
pub(crate) fn literal_entry(path: &str) -> String {
    if is_pattern_entry(path) || path.starts_with(LITERAL_PREFIX) {
        format!("{LITERAL_PREFIX}{path}")
    } else {
        path.to_owned()
    }
}

pub(crate) fn parse_fs_rule(entry: &str, base: &Path) -> Result<FsRule, String> {
    parse_with_home(entry, base, env::var_os("HOME").map(PathBuf::from))
}

pub(crate) fn parse_fs_rules(entries: &[String], base: &Path) -> Result<Vec<FsRule>, String> {
    entries
        .iter()
        .map(|entry| parse_fs_rule(entry, base))
        .collect()
}

/// Concrete paths for backends that can't match patterns: plain paths as-is,
/// patterns expanded against the filesystem as it is now. When `scope` is set,
/// pattern walks are limited to it (paths outside are invisible anyway).
pub(crate) fn expand_fs_rules(rules: &[FsRule], scope: Option<&Path>) -> Vec<String> {
    let mut paths = Vec::new();
    for rule in rules {
        match rule {
            FsRule::Path(path) => paths.push(path.clone()),
            FsRule::Pattern {
                source,
                compiled,
                root,
                ..
            } => {
                let start = match scope {
                    None => root.clone(),
                    Some(scope) if scope.starts_with(root) => scope.to_path_buf(),
                    Some(scope) if root.starts_with(scope) => root.clone(),
                    Some(_) => continue,
                };
                expand_pattern(source, compiled, &start, &mut paths);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Parses and expands in one step; see `expand_fs_rules`.
pub(crate) fn expand_policy_entries(
    entries: &[String],
    base: &Path,
    scope: Option<&Path>,
) -> Result<Vec<String>, String> {
    Ok(expand_fs_rules(&parse_fs_rules(entries, base)?, scope))
}

fn expand_pattern(source: &str, compiled: &regex::Regex, start: &Path, out: &mut Vec<String>) {
    let matches = |path: &Path| compiled.is_match(&path.to_string_lossy());
    if matches(start) {
        out.push(start.to_string_lossy().into_owned());
        return;
    }
    let mut stack = vec![start.to_path_buf()];
    let mut visited = 0usize;
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                eprintln!(
                    "warning: stopped expanding filesystem pattern '{source}' after {MAX_WALK_ENTRIES} entries under {}; narrow the pattern so it is fully enforced",
                    start.display()
                );
                return;
            }
            let path = entry.path();
            if matches(&path) {
                // A matched directory is denied as a whole; no need to descend.
                out.push(path.to_string_lossy().into_owned());
            } else if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path);
            }
        }
    }
}

fn parse_with_home(entry: &str, base: &Path, home: Option<PathBuf>) -> Result<FsRule, String> {
    let entry = entry.trim();
    if let Some(literal) = entry.strip_prefix(LITERAL_PREFIX) {
        return Ok(FsRule::Path(resolve_path(literal, base, home.as_deref())));
    }
    if is_pattern_entry(entry) && cfg!(windows) {
        // The regexes assume `/` separators; on Windows they would silently
        // match nothing, so fail closed instead.
        return Err(format!(
            "'{entry}': glob and regex filesystem entries are not supported on Windows"
        ));
    }
    if let Some(body) = entry.strip_prefix(REGEX_PREFIX) {
        return parse_regex(entry, body, home.as_deref());
    }
    if entry.contains(['*', '?']) {
        return parse_glob(entry, base, home.as_deref());
    }
    Ok(FsRule::Path(resolve_path(entry, base, home.as_deref())))
}

/// Resolves `~`-prefixed and relative paths to absolute ones; relative paths
/// are joined onto `base` (normally the process's cwd, but the worktree
/// directory for a `--worktree` run).
fn resolve_path(path: &str, base: &Path, home: Option<&Path>) -> String {
    let path = path.trim();
    let path = if path == "~" {
        home.map_or_else(|| PathBuf::from(path), Path::to_path_buf)
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.map_or_else(|| PathBuf::from("~"), Path::to_path_buf)
            .join(rest)
    } else {
        PathBuf::from(path)
    };
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    path.to_string_lossy().into_owned()
}

fn parse_glob(entry: &str, base: &Path, home: Option<&Path>) -> Result<FsRule, String> {
    let (prefix, glob) = if let Some(rest) = entry.strip_prefix("~/") {
        let home = home.ok_or_else(|| format!("'{entry}' uses '~' but HOME is not set"))?;
        (home.to_string_lossy().into_owned(), rest)
    } else if entry.starts_with('/') {
        (String::new(), entry)
    } else {
        (base.to_string_lossy().into_owned(), entry)
    };
    let prefix = prefix.trim_end_matches('/');

    let segments = glob
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();
    if segments.contains(&"..") {
        return Err(format!("'{entry}' must not contain '..' segments"));
    }
    if segments.is_empty() {
        return Err(format!("'{entry}' does not name any path"));
    }

    let mut regex = format!("^{}", escape_regex(prefix));
    let mut root = PathBuf::from(if prefix.is_empty() { "/" } else { prefix });
    let mut literal_so_far = true;
    let last = segments.len() - 1;
    for (index, segment) in segments.iter().enumerate() {
        if *segment == "**" {
            literal_so_far = false;
            // `**/` matches zero or more whole directories; a trailing `**`
            // matches everything below.
            regex.push_str(if index == last { "/.*" } else { "(/.*)?" });
            continue;
        }
        regex.push('/');
        let translated = translate_segment(entry, segment)?;
        if literal_so_far && index != last && translated.is_none() {
            root.push(segment);
        } else {
            literal_so_far = false;
        }
        regex.push_str(&translated.unwrap_or_else(|| escape_regex(segment)));
    }
    // Like a plain path, a pattern that matches a directory covers its contents.
    regex.push_str("(/.*)?$");
    compile(entry, regex, root)
}

/// Returns `None` for a segment without wildcards.
fn translate_segment(entry: &str, segment: &str) -> Result<Option<String>, String> {
    if !segment.contains(['*', '?', '[']) {
        return Ok(None);
    }
    let chars = segment.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' => {
                while chars.get(index + 1) == Some(&'*') {
                    index += 1;
                }
                out.push_str("[^/]*");
            }
            '?' => out.push_str("[^/]"),
            '[' => match glob_class(&chars[index..]) {
                Some((class, consumed)) => {
                    validate_class(entry, &class)?;
                    out.push_str(&class);
                    index += consumed - 1;
                }
                None => out.push_str("\\["),
            },
            other => out.push_str(&escape_regex(&other.to_string())),
        }
        index += 1;
    }
    Ok(Some(out))
}

/// Converts a glob class starting at `chars[0] == '['` to a regex class that
/// never matches `/`. Returns the class and how many chars it consumed, or
/// `None` if the bracket is unterminated (it's then a literal `[`).
fn glob_class(chars: &[char]) -> Option<(String, usize)> {
    let mut index = 1;
    let negated = matches!(chars.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let body_start = index;
    // A `]` right after the opening bracket is a literal member.
    if chars.get(index) == Some(&']') {
        index += 1;
    }
    while index < chars.len() && chars[index] != ']' {
        index += 1;
    }
    if index >= chars.len() {
        return None;
    }
    let body = chars[body_start..index].iter().collect::<String>();
    let class = if negated {
        format!("[^/{body}]")
    } else {
        format!("[{body}]")
    };
    Some((class, index + 1))
}

fn validate_class(entry: &str, class: &str) -> Result<(), String> {
    let body = &class[1..class.len() - 1];
    if body.contains(['\\', '[']) || ["&&", "--", "~~"].iter().any(|op| body.contains(op)) {
        return Err(format!(
            "'{entry}' has an unsupported character class '{class}'; use plain characters and ranges like [a-z]"
        ));
    }
    Ok(())
}

fn parse_regex(entry: &str, body: &str, home: Option<&Path>) -> Result<FsRule, String> {
    let body = if let Some(rest) = body.strip_prefix("^~/") {
        let home = home.ok_or_else(|| format!("'{entry}' uses '~' but HOME is not set"))?;
        format!(
            "^{}/{rest}",
            escape_regex(home.to_string_lossy().trim_end_matches('/'))
        )
    } else if body.starts_with("^/") {
        body.to_owned()
    } else {
        return Err(format!(
            "'{entry}' must start with '^/' or '^~/' so it is anchored to an absolute path"
        ));
    };
    for unsupported in [
        "\\d", "\\D", "\\w", "\\W", "\\s", "\\S", "\\b", "\\B", "\\A", "\\z", "\\p", "\\P", "(?",
    ] {
        if body.contains(unsupported) {
            return Err(format!(
                "'{entry}' uses '{unsupported}', which macOS Seatbelt does not support; use POSIX forms like [0-9] or [a-zA-Z_]"
            ));
        }
    }
    let root = regex_root(entry, &body)?;
    compile(entry, cover_descendants(&body), root)
}

/// Rewrites an end-anchored regex so a matched directory also covers its
/// contents, as it does for plain paths and globs: `^/a$` becomes
/// `^(/a)(/.*)?$`. Without a trailing `$` the regex already matches every
/// path it is a prefix of. Grouping is safe because `regex_root` rejects
/// top-level alternation.
fn cover_descendants(body: &str) -> String {
    let inner = &body[1..];
    match inner.strip_suffix('$') {
        Some(inner)
            if inner
                .chars()
                .rev()
                .take_while(|character| *character == '\\')
                .count()
                % 2
                == 0 =>
        {
            format!("^({inner})(/.*)?$")
        }
        _ => body.to_owned(),
    }
}

/// The literal directory prefix of an anchored regex. Rejects top-level
/// alternation, which would make the leading `^/...` apply to one branch only.
fn regex_root(entry: &str, body: &str) -> Result<PathBuf, String> {
    let chars = body.chars().collect::<Vec<_>>();
    let mut depth = 0usize;
    let mut index = 1;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            '[' => {
                while index < chars.len() && chars[index] != ']' {
                    index += 1;
                }
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                return Err(format!(
                    "'{entry}' uses top-level '|'; group the alternatives, e.g. ^/(a|b)/..."
                ))
            }
            _ => {}
        }
        index += 1;
    }

    let mut literal = String::new();
    for character in chars.iter().skip(1) {
        match character {
            '?' | '*' | '{' => {
                // The quantifier makes the preceding character optional.
                literal.pop();
                break;
            }
            '.' | '[' | ']' | '(' | ')' | '+' | '|' | '^' | '$' | '\\' => break,
            other => literal.push(*other),
        }
    }
    let root = match literal.rfind('/') {
        Some(0) | None => "/",
        Some(end) => &literal[..end],
    };
    Ok(PathBuf::from(root))
}

fn compile(entry: &str, regex: String, root: PathBuf) -> Result<FsRule, String> {
    let compiled = regex::Regex::new(&regex)
        .map_err(|error| format!("'{entry}' is not a valid pattern: {error}"))?;
    Ok(FsRule::Pattern {
        source: entry.to_owned(),
        regex,
        compiled,
        root,
    })
}

/// Backslash-escapes regex metacharacters, using only escapes that mean the
/// same thing to Seatbelt and to the `regex` crate.
fn escape_regex(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if ".+*?()|[]{}^$\\".contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::{expand_fs_rules, literal_entry, parse_with_home, FsRule};
    use std::path::{Path, PathBuf};

    fn rule(entry: &str) -> FsRule {
        parse_with_home(
            entry,
            Path::new("/work/repo"),
            Some(PathBuf::from("/home/u")),
        )
        .unwrap()
    }

    fn error(entry: &str) -> String {
        parse_with_home(
            entry,
            Path::new("/work/repo"),
            Some(PathBuf::from("/home/u")),
        )
        .unwrap_err()
    }

    fn matches(entry: &str, path: &str) -> bool {
        rule(entry).matches(Path::new(path))
    }

    fn root(entry: &str) -> PathBuf {
        match rule(entry) {
            FsRule::Pattern { root, .. } => root,
            FsRule::Path(_) => panic!("'{entry}' parsed as a plain path"),
        }
    }

    #[test]
    fn plain_paths_resolve_home_and_relative_entries() {
        assert!(matches!(rule("secrets"), FsRule::Path(p) if p == "/work/repo/secrets"));
        assert!(matches!(rule("~/.ssh"), FsRule::Path(p) if p == "/home/u/.ssh"));
        assert!(matches!(rule("~"), FsRule::Path(p) if p == "/home/u"));
        assert!(matches("~/.ssh", "/home/u/.ssh/id_ed25519"));
        assert!(!matches("~/.ssh", "/home/u/.sshx"));
    }

    #[test]
    fn brackets_alone_keep_an_entry_a_plain_path() {
        assert!(matches!(rule("/notes/[old]"), FsRule::Path(p) if p == "/notes/[old]"));
        assert!(matches!(rule("path:/a/*b?"), FsRule::Path(p) if p == "/a/*b?"));
        assert_eq!(literal_entry("/plain"), "/plain");
        assert_eq!(literal_entry("/odd/a?b"), "path:/odd/a?b");
        assert_eq!(literal_entry("re:x"), "path:re:x");
    }

    #[test]
    fn single_star_and_question_mark_stay_within_a_segment() {
        assert!(matches("/a/*.pem", "/a/key.pem"));
        assert!(matches("/a/*.pem", "/a/.hidden.pem"));
        assert!(!matches("/a/*.pem", "/a/b/key.pem"));
        assert!(matches("/a/file?.txt", "/a/file1.txt"));
        assert!(!matches("/a/file?.txt", "/a/file10.txt"));
        assert!(!matches("/a/x?y", "/a/x/y"));
    }

    #[test]
    fn double_star_matches_zero_or_more_directories() {
        assert!(matches("**/.env*", "/work/repo/.env"));
        assert!(matches("**/.env*", "/work/repo/a/b/.env.local"));
        assert!(!matches("**/.env*", "/work/other/.env"));
        assert!(matches("/a/**", "/a/b/c"));
        assert!(!matches("/a/**", "/a"));
    }

    #[test]
    fn a_matched_directory_covers_its_contents() {
        assert!(matches("/a/secret*", "/a/secrets/deep/file"));
    }

    #[test]
    fn character_classes_exclude_slash_and_support_negation() {
        assert!(matches("/a/key[0-9]*", "/a/key7.pem"));
        assert!(!matches("/a/key[0-9]*", "/a/keyX.pem"));
        assert!(matches("/a/[!.]*", "/a/visible"));
        assert!(!matches("/a/[!.]*", "/a/.hidden"));
        assert!(error("/a/[\\d]*").contains("unsupported character class"));
    }

    #[test]
    fn regex_metacharacters_in_globs_are_literal() {
        assert!(matches("/a/x.y+(z)$*", "/a/x.y+(z)$1"));
        assert!(!matches("/a/x.y*", "/a/xzy1"));
    }

    #[test]
    fn glob_roots_stop_at_the_first_wildcard_segment() {
        assert_eq!(root("**/.env*"), PathBuf::from("/work/repo"));
        assert_eq!(root("~/.config/*/token"), PathBuf::from("/home/u/.config"));
        assert_eq!(root("/*"), PathBuf::from("/"));
    }

    #[test]
    fn glob_rejects_parent_segments() {
        assert!(error("../*.env").contains("'..'"));
    }

    #[test]
    fn regex_entries_are_anchored_and_expand_home() {
        assert!(matches("re:^~/.*\\.pem$", "/home/u/certs/a.pem"));
        assert!(!matches("re:^~/.*\\.pem$", "/other/a.pem"));
        assert_eq!(root("re:^~/.*\\.pem$"), PathBuf::from("/home/u"));
        assert_eq!(root("re:^/var/lib/app[0-9]/x"), PathBuf::from("/var/lib"));
        assert_eq!(root("re:^/var/lib/?x"), PathBuf::from("/var"));
        assert!(error("re:.*\\.pem$").contains("must start with"));
        assert!(error("re:^/a/\\d+").contains("Seatbelt"));
        assert!(error("re:^/a|^/b").contains("top-level '|'"));
        assert!(error("re:^/a/(").contains("not a valid pattern"));
        assert!(matches("re:^/(a|b)/x", "/b/x"));
    }

    #[test]
    fn end_anchored_regexes_still_cover_directory_contents() {
        assert!(matches("re:^/tmp/secrets$", "/tmp/secrets"));
        assert!(matches("re:^/tmp/secrets$", "/tmp/secrets/key"));
        assert!(matches("re:^/tmp/secrets$", "/tmp/secrets/deep/key"));
        assert!(!matches("re:^/tmp/secrets$", "/tmp/secretsX"));
        assert!(matches("re:^~/.*\\.pem$", "/home/u/certs/a.pem"));
        assert!(!matches("re:^~/.*\\.pem$", "/home/u/certs/a.pemX"));
        // An escaped `$` is a literal character, not an end anchor.
        assert!(matches("re:^/a/b\\$", "/a/b$"));
        assert!(!matches("re:^/a/b\\$", "/a/b"));
        match rule("re:^/tmp/secrets$") {
            FsRule::Pattern { regex, .. } => assert_eq!(regex, "^(/tmp/secrets)(/.*)?$"),
            FsRule::Path(_) => panic!("parsed as a plain path"),
        }
    }

    #[test]
    fn expansion_finds_hidden_and_ignored_files_without_descending_matches() {
        let tree =
            std::env::temp_dir().join(format!("stashbase-fs-rules-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(tree.join("sub/.env.d/inner")).unwrap();
        std::fs::write(tree.join(".gitignore"), ".env\n").unwrap();
        std::fs::write(tree.join(".env"), "A=1").unwrap();
        std::fs::write(tree.join("sub/.env.local"), "B=2").unwrap();
        std::fs::write(tree.join("sub/.env.d/inner/.env"), "C=3").unwrap();
        std::fs::write(tree.join("sub/readme"), "").unwrap();

        let rules = vec![
            parse_with_home("**/.env*", &tree, None).unwrap(),
            parse_with_home("missing/**/*.pem", &tree, None).unwrap(),
        ];
        let expected = [".env", "sub/.env.d", "sub/.env.local"]
            .iter()
            .map(|path| tree.join(path).to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(expand_fs_rules(&rules, None), expected);
        assert_eq!(
            expand_fs_rules(&rules, Some(&tree.join("sub"))),
            expected[1..]
        );
        assert!(expand_fs_rules(&rules, Some(Path::new("/nonexistent-scope"))).is_empty());

        std::fs::remove_dir_all(tree).unwrap();
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::{literal_entry, parse_fs_rule, FsRule};
    use std::path::Path;

    #[test]
    fn patterns_fail_closed_but_plain_paths_still_work() {
        let base = Path::new(r"C:\work\repo");
        for entry in ["**/.env*", "secrets/*.pem", "re:^/work/.*"] {
            let error = parse_fs_rule(entry, base).unwrap_err();
            assert!(
                error.contains("not supported on Windows"),
                "{entry}: {error}"
            );
        }
        assert!(matches!(
            parse_fs_rule(".env", base),
            Ok(FsRule::Path(path)) if path == r"C:\work\repo\.env"
        ));
        assert!(matches!(
            parse_fs_rule(&literal_entry(r"C:\odd\a*b"), base),
            Ok(FsRule::Path(path)) if path == r"C:\odd\a*b"
        ));
    }
}
