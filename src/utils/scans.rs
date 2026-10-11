#![allow(dead_code)]

use crate::models::{
    scans::{
        ChangeRangeWithHash, DiffHunk, FileChangesScanResponse, MatchedFileSecret, MatchedSecrets,
        ScanFinding,
    },
    validation::ScanInputValidationError,
};
use anyhow::Result;
use git2;
use ignore::gitignore::GitignoreBuilder;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

pub static SCAN_IGNORE_LINE_COMMENT: &str = "@stashbase-ignore";
pub static SCAN_CONTEXT_LINES: usize = 10;

const DEFAULT_EXCLUDE_FILE_NAMES: [&str; 4] =
    [".gitignore", ".gitattributes", ".gitmodules", ".gitkeep"];
const DEFAULT_EXCLUDE_DIRS: [&str; 3] = ["node_modules/", "vendor/", "vendors/"];
const DEFAULT_EXCLUDE_FILE_PATTERNS: [&str; 3] = ["top-1000.txt", "*.sops", "*.sops.yaml"];
const DEFAULT_EXCLUDE_FILE_EXTENSIONS: [&str; 5] = ["html", "css", "lock", "storyboard", "xib"];

pub fn default_scan_exclude_patterns() -> Vec<String> {
    let mut patterns = Vec::new();
    patterns.extend(
        DEFAULT_EXCLUDE_FILE_NAMES
            .iter()
            .map(|name| name.to_string()),
    );
    patterns.extend(DEFAULT_EXCLUDE_DIRS.iter().map(|dir| dir.to_string()));
    patterns.extend(
        DEFAULT_EXCLUDE_FILE_PATTERNS
            .iter()
            .map(|pattern| pattern.to_string()),
    );
    patterns.extend(
        DEFAULT_EXCLUDE_FILE_EXTENSIONS
            .iter()
            .map(|ext| format!("*.{}", ext)),
    );
    patterns
}

pub fn should_merge_hunks(hunk1: &DiffHunk, hunk2: &DiffHunk, max_gap: usize) -> bool {
    // Only merge if they're close enough
    if (hunk2.start_line as i64 - hunk1.end_line as i64).abs() > max_gap as i64 {
        return false;
    }

    // Check for context overlap
    hunk1.end_line >= hunk2.start_line || (hunk2.start_line - hunk1.end_line) <= max_gap
}

pub fn get_comment_prefix(extension: &str) -> Option<&'static str> {
    let extension = extension.to_lowercase();
    match extension.as_str() {
        "rs" | "js" | "ts" | "tsx" | "jsx" | "vue" | "java" | "cpp" | "c" | "cs" | "go "
        | "php" | "kt" | "scala" | "dart" | "svelte" | "m" | "mm" => Some("//"),
        "py" | "sh" | "toml" | "yaml" | "yml" | "ini" | "r" | "swift" | "rb" | "dockerfile"
        | "makefile" | "ex" | "es" | "exs" | "pl" => Some("#"),
        "sql" | "hs" | "lua" => Some("--"),
        _ => None,
    }
}

pub fn is_comment_line(line: &str, comment_prefix: &str) -> bool {
    line.trim_start().starts_with(comment_prefix)
}

pub fn should_skip_line(line: &str, comment_prefix: &str, skip_comment: &str) -> bool {
    if is_comment_line(line, comment_prefix) {
        let trimmed = line.trim_start();

        if trimmed.starts_with(comment_prefix) {
            let without_prefix = trimmed.trim_start_matches(comment_prefix).trim_start();
            // println!("without_prefix: {}", without_prefix);

            if without_prefix == String::from(skip_comment) {
                return true;
            }
        }
    }

    false
}

pub fn is_binary_file(extension: &str) -> bool {
    let extension = extension.to_lowercase();
    match extension.as_str() {
        // Compiled files
        "exe" | "dll" | "so" | "dylib" | "class" | "o" | "obj" | "pyc" | "pyo" |

        // Compressed archives
        "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" | "xz" |

        // Media files
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "ico" | "webp" | "tiff" | // Images
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "wma" | // Audio
        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" | // Video

        // Document formats
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" |

        // Database files
        "db" | "sqlite" | "mdb" | "frm" | "myd" | "myi" |

        // Other binary formats
        "bin" | "iso" | "img" | "dat" => true,

        // Everything else is considered text
        _ => false,
    }
}

pub fn should_exclude_file(
    file_path: &str,
    exclude_patterns: &[String],
) -> Result<bool, ScanInputValidationError> {
    let mut builder = GitignoreBuilder::new("/"); // Root directory

    for pattern in exclude_patterns {
        builder.add_line(None, pattern).map_err(|e| {
            ScanInputValidationError::InvalidExcludePattern {
                pattern: pattern.clone(),
                message: e.to_string(),
            }
        })?;
    }

    let gitignore =
        builder
            .build()
            .map_err(|e| ScanInputValidationError::GitignoreBuilderError {
                message: e.to_string(),
            })?;

    Ok(gitignore.matched(Path::new(file_path), false).is_ignore())
}

pub fn calculate_hash(content: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.finalize().to_vec()
}

pub fn get_latest_scan_file(output_dir: &str) -> Option<std::fs::DirEntry> {
    let scan_dir = Path::new(output_dir);
    fs::read_dir(scan_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .max_by_key(|entry| entry.path())
}

pub fn file_content_equals(file_path: &str, new_content: &str) -> bool {
    match fs::read_to_string(file_path) {
        Ok(content) => {
            let existing_hash = calculate_hash(&content);
            let new_hash = calculate_hash(new_content);
            new_hash == existing_hash
        }
        Err(_) => false,
    }
}

pub fn save_scan_results(output_dir: &str, json_content: &str) -> Result<String> {
    // Create scan_results directory if it doesn't exist
    fs::create_dir_all(output_dir)?;

    // Get current timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create file path
    let file_path = format!("{}/{}.json", output_dir, timestamp);

    // Write to file
    fs::write(&file_path, json_content)?;

    Ok(file_path)
}

pub fn filter_sha256_hashes(hashes: Vec<String>) -> Vec<String> {
    hashes
        .into_iter()
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .collect::<Vec<_>>()
}

pub fn is_valid_sha256_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn load_baseline_results(
    baseline_path: &str,
) -> Result<Vec<ScanFinding>, ScanInputValidationError> {
    let content = fs::read_to_string(baseline_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScanInputValidationError::BaselineFileNotFound {
                path: baseline_path.to_string(),
            }
        } else {
            ScanInputValidationError::BaselineFileRead {
                path: baseline_path.to_string(),
                message: e.to_string(),
            }
        }
    })?;

    let baseline_response: FileChangesScanResponse =
        serde_json::from_str(&content).map_err(|e| {
            ScanInputValidationError::BaselineFileParse {
                path: baseline_path.to_string(),
                message: e.to_string(),
            }
        })?;

    Ok(baseline_response.findings)
}

pub fn compute_finding_hash(finding: &ScanFinding) -> String {
    let mut hasher = Sha256::new();
    hasher.update(finding.file_path.as_bytes());
    for location in &finding.locations {
        hasher.update(location.start_line.to_string().as_bytes());
        hasher.update(location.end_line.to_string().as_bytes());
    }
    hasher.update(finding.value_sha256.as_bytes());
    hasher.update(finding.preview.as_bytes());
    hasher.update(finding.severity.to_string().as_bytes());

    if let Some(commit_id) = &finding.commit_sha {
        hasher.update(commit_id.as_bytes());
    }

    format!("{:x}", hasher.finalize())
}

pub fn filter_new_findings(
    current_findings: Vec<ScanFinding>,
    baseline_findings: Vec<ScanFinding>,
) -> Vec<ScanFinding> {
    let baseline_hashes: HashSet<_> = baseline_findings
        .iter()
        .map(|finding| compute_finding_hash(finding))
        .collect();

    let filtered_findings = current_findings
        .into_iter()
        .filter(|finding| !baseline_hashes.contains(&compute_finding_hash(finding)))
        .collect::<Vec<_>>();

    let mut sorted_findings: Vec<_> = filtered_findings.into_iter().collect();

    sorted_findings.sort_by(|a, b| {
        let a_start_line = a
            .first_location()
            .map(|range| range.start_line)
            .unwrap_or(0);
        let b_start_line = b
            .first_location()
            .map(|range| range.start_line)
            .unwrap_or(0);

        (b.severity.clone() as i32)
            .cmp(&(a.severity.clone() as i32)) // by severity, descending
            .then(a.file_path.cmp(&b.file_path)) // then by file path
            .then(a_start_line.cmp(&b_start_line)) // then by start line
    });

    sorted_findings
}

pub fn process_diff_line(
    line: git2::DiffLine,
    file_path: &str,
    is_new_file: bool,
    current_changes: &mut Option<ChangeRangeWithHash>,
    last_hunk: &mut DiffHunk,
    prev_line: &mut String,
    ignore_line_comment: &str,
    context_lines: usize,
) -> bool {
    let line_number = line.new_lineno().unwrap_or(0) as usize;
    let content = String::from_utf8_lossy(line.content()).to_string();
    let content_hash: [u8; 32] = sha2::Sha256::digest(content.as_bytes()).into();

    // Skip "No newline at end of file" messages
    if content.trim() == "\\ No newline at end of file" {
        return true;
    }

    let path = Path::new(file_path);
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    // Add to full_content if it's a context line (not '-') or it's an addition
    if (context_lines > 0 && line.origin() != '-') || line.origin() == '+' {
        last_hunk.full_content.push_str(&content);
    }

    // For new files, update the end line number and ensure changes is None
    if is_new_file {
        last_hunk.end_line = line_number;
        last_hunk.changes = None;
        *current_changes = None;
        // Update previous line content and return early for new files
        if line.origin() != '-' {
            *prev_line = content;
        }
        return true;
    }

    // Initialize changes as Vec if None (for modified files)
    if last_hunk.changes.is_none() {
        last_hunk.changes = Some(Vec::new());
    }

    // Check for removed ignore comments
    if line.origin() == '-' {
        if let Some(comment_prefix) = get_comment_prefix(extension) {
            let line_without_comment_prefix =
                content.trim().trim_start_matches(comment_prefix).trim();

            if line_without_comment_prefix.starts_with(ignore_line_comment) {
                // This is a removed ignore comment - treat it as a change
                let actual_line = line.old_lineno().unwrap_or(0) as usize;

                match current_changes {
                    Some(ref mut change) => {
                        // For removed lines, we need to ensure proper line number tracking
                        if actual_line >= change.start_line && actual_line <= change.end_line + 3 {
                            change.end_line = std::cmp::max(change.end_line, actual_line);
                            change.content_hash = content_hash;
                        } else {
                            // Check if this content already exists in the hunk's changes
                            let content_exists = last_hunk
                                .changes
                                .as_ref()
                                .map(|changes| {
                                    changes
                                        .iter()
                                        .any(|change| change.content_hash == content_hash)
                                })
                                .unwrap_or(false);

                            if !content_exists {
                                if let Some(changes) = &mut last_hunk.changes {
                                    changes.push(change.clone());
                                }

                                let change_range = ChangeRangeWithHash {
                                    start_line: actual_line,
                                    end_line: actual_line,
                                    content_hash: content_hash,
                                };

                                *current_changes = Some(change_range);
                            }
                        }
                    }
                    None => {
                        // Check if this content already exists in the hunk's changes
                        let content_exists = last_hunk
                            .changes
                            .as_ref()
                            .map(|changes| {
                                changes
                                    .iter()
                                    .any(|change| change.content_hash == content_hash)
                            })
                            .unwrap_or(false);

                        if !content_exists {
                            let change_range = ChangeRangeWithHash {
                                start_line: actual_line,
                                end_line: actual_line,
                                content_hash: content_hash,
                            };

                            *current_changes = Some(change_range);
                        }
                    }
                }
            }
        }
    }

    // Handle changes for modified files
    if line.origin() == '+' {
        // Check if previous line has a skip comment
        let should_skip = if let Some(comment_prefix) = get_comment_prefix(extension) {
            let prev = prev_line.trim().to_string();
            let should_skip = should_skip_line(&prev, comment_prefix, ignore_line_comment);
            let line_without_comment_prefix =
                content.trim().trim_start_matches(comment_prefix).trim();

            should_skip || line_without_comment_prefix.starts_with(ignore_line_comment)
        } else {
            false
        };

        let is_blank_line = content.trim().is_empty();

        if !should_skip {
            match current_changes {
                Some(ref mut change) => {
                    // Continue existing change if it's within reasonable range
                    if line_number <= change.end_line + 3 {
                        // Always include the line if we're in the middle of a change
                        change.end_line = std::cmp::max(change.end_line, line_number);
                        change.content_hash = content_hash;
                    } else {
                        // Check if this content already exists in the hunk's changes
                        let content_exists = last_hunk
                            .changes
                            .as_ref()
                            .map(|changes| {
                                changes
                                    .iter()
                                    .any(|change| change.content_hash == content_hash)
                            })
                            .unwrap_or(false);

                        if !content_exists {
                            // Gap too large, create new change range
                            if let Some(changes) = &mut last_hunk.changes {
                                changes.push(change.clone());
                            }
                            // Don't start new change if it's a blank line
                            if !is_blank_line {
                                let change_range = ChangeRangeWithHash {
                                    start_line: line_number,
                                    end_line: line_number,
                                    content_hash: content_hash,
                                };

                                *current_changes = Some(change_range);
                            }
                        }
                    }
                }
                None => {
                    // Don't start new change if it's a blank line
                    if !is_blank_line {
                        // Check if this content already exists in the hunk's changes
                        let content_exists = last_hunk
                            .changes
                            .as_ref()
                            .map(|changes| {
                                changes
                                    .iter()
                                    .any(|change| change.content_hash == content_hash)
                            })
                            .unwrap_or(false);

                        if !content_exists {
                            let change_range = ChangeRangeWithHash {
                                start_line: line_number,
                                end_line: line_number,
                                content_hash: content_hash,
                            };

                            *current_changes = Some(change_range);
                        }
                    }
                }
            }
        }
    }

    // Update previous line content
    if line.origin() != '-' {
        *prev_line = content;
    }

    true
}

// find matched secrets in a files
// retruns hashmap of file path to a vector of (secret name, secret value hash)
pub fn get_file_matches(
    files: Vec<String>,
    findings: Vec<ScanFinding>,
) -> HashMap<String, Vec<(String, String)>> {
    use crate::{cmd::secrets::SecretsFileFormat, utils::secrets::read_secrets_from_file};
    use std::path::Path;

    let mut matches: HashMap<String, Vec<(String, String)>> = HashMap::with_capacity(files.len());

    // Create a set of finding value hashes for quick lookup
    let finding_hashes: HashSet<String> = findings
        .iter()
        .map(|finding| finding.value_sha256.clone())
        .collect();

    // Early exit if no findings to match against
    if finding_hashes.is_empty() {
        return matches;
    }

    for file_path in files {
        let path = Path::new(&file_path);

        if !path.exists() {
            continue;
        }

        // Determine format from file extension
        let target_format = if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
            SecretsFileFormat::Yaml
        } else if file_path.ends_with(".json") {
            SecretsFileFormat::Json
        } else {
            SecretsFileFormat::Dotenv
        };

        // Try to read secrets from file
        if let Ok(secrets) = read_secrets_from_file(path, &target_format) {
            let mut file_matches = Vec::with_capacity(secrets.len().min(finding_hashes.len()));

            for secret in secrets {
                // Hash the secret value using SHA256
                let mut hasher = Sha256::new();
                hasher.update(secret.value.as_bytes());
                let value_hash = format!("{:x}", hasher.finalize());

                // Check if this hash matches any finding
                if finding_hashes.contains(&value_hash) {
                    file_matches.push((secret.name, value_hash));
                }
            }

            if !file_matches.is_empty() {
                matches.insert(file_path, file_matches);
            }
        }
    }

    matches
}

pub fn update_findings_with_file_matches(
    findings: &mut Vec<ScanFinding>,
    file_matches: HashMap<String, Vec<(String, String)>>,
) {
    // Build a reverse index: value_hash -> Vec<(file_path, secret_name)>
    let mut hash_to_matches: HashMap<String, Vec<(String, String)>> =
        HashMap::with_capacity(file_matches.len());

    for (file_path, matches) in file_matches {
        for (secret_name, value_hash) in matches {
            hash_to_matches
                .entry(value_hash)
                .or_insert_with(Vec::new)
                .push((file_path.clone(), secret_name));
        }
    }

    // Process each finding once using the reverse index
    for finding in findings.iter_mut() {
        if let Some(matches) = hash_to_matches.get(&finding.value_sha256) {
            // Ensure matched_secrets is initialized
            let matched_secrets = finding
                .matched_secrets
                .get_or_insert_with(|| MatchedSecrets {
                    project: None,
                    files: Some(Vec::new()),
                });

            // Ensure files is initialized
            let files = matched_secrets.files.get_or_insert_with(Vec::new);

            // Group matches by secret_name to batch file paths and avoid duplicate processing
            let mut secret_to_files: HashMap<&str, HashSet<&str>> = HashMap::new();
            for (file_path, secret_name) in matches {
                secret_to_files
                    .entry(secret_name)
                    .or_insert_with(HashSet::new)
                    .insert(file_path);
            }

            // Update or create MatchedFileSecret entries
            for (secret_name, file_paths_set) in secret_to_files {
                // Find existing secret by name
                if let Some(existing_secret) =
                    files.iter_mut().find(|f| f.secret_name == secret_name)
                {
                    // Secret exists, merge file paths
                    for file_path in file_paths_set {
                        let file_path_string = file_path.to_string();
                        if !existing_secret.file_paths.contains(&file_path_string) {
                            existing_secret.file_paths.push(file_path_string);
                        }
                    }
                } else {
                    // Secret doesn't exist, create new entry
                    files.push(MatchedFileSecret {
                        secret_name: secret_name.to_string(),
                        file_paths: file_paths_set.into_iter().map(|s| s.to_string()).collect(),
                    });
                }
            }
        }
    }
}

pub const RESTRICTED_SCAN_MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const RESTRICTED_SCAN_MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub const RESTRICTED_SCAN_MAX_FILES: usize = 10_000;
pub const RESTRICTED_SCAN_MAX_COMMITS: usize = 1_000;

/// Caps what a restricted scan reads. The Agent Proxy runs it on the host for
/// a sandboxed agent, which controls the staged content and the history, so
/// without a cap it could make the host load arbitrarily large blobs.
pub struct RestrictedScanBudget {
    remaining_bytes: u64,
    remaining_files: usize,
    remaining_commits: usize,
}

impl RestrictedScanBudget {
    pub fn new() -> Self {
        Self {
            remaining_bytes: RESTRICTED_SCAN_MAX_TOTAL_BYTES,
            remaining_files: RESTRICTED_SCAN_MAX_FILES,
            remaining_commits: RESTRICTED_SCAN_MAX_COMMITS,
        }
    }

    /// `Some` when this process is a restricted scan.
    pub fn for_current_scan() -> Option<Self> {
        (std::env::var(crate::models::scans::SCAN_RESTRICTED_ENV).as_deref() == Ok("1"))
            .then(Self::new)
    }

    /// Keeps libgit2 from loading a blob above the per-file cap, as a second
    /// line behind `charge_diff`.
    pub fn limit_diff_options(options: &mut git2::DiffOptions) {
        options.max_size(RESTRICTED_SCAN_MAX_FILE_BYTES as i64);
    }

    pub fn charge_commit(&mut self) -> Result<(), ScanInputValidationError> {
        if self.remaining_commits == 0 {
            return Err(too_large(format!(
                "more than {RESTRICTED_SCAN_MAX_COMMITS} commits to scan"
            )));
        }
        self.remaining_commits -= 1;
        Ok(())
    }

    /// Charges every changed file in `diff` before any content is loaded.
    /// Sizes come from the object headers, which the agent cannot misstate
    /// the way it can an index entry.
    pub fn charge_diff(
        &mut self,
        repo: &git2::Repository,
        diff: &git2::Diff,
    ) -> Result<(), ScanInputValidationError> {
        let odb = repo
            .odb()
            .map_err(|e| ScanInputValidationError::GitDiffProcessing {
                message: e.message().to_string(),
            })?;
        for delta in diff.deltas() {
            if delta.status() == git2::Delta::Deleted {
                continue;
            }
            if self.remaining_files == 0 {
                return Err(too_large(format!(
                    "more than {RESTRICTED_SCAN_MAX_FILES} changed files"
                )));
            }
            self.remaining_files -= 1;

            let file = delta.new_file();
            if file.id().is_zero() {
                continue;
            }
            let (size, _) = odb.read_header(file.id()).map_err(|e| {
                ScanInputValidationError::GitDiffProcessing {
                    message: e.message().to_string(),
                }
            })?;
            let size = size as u64;
            if size > RESTRICTED_SCAN_MAX_FILE_BYTES {
                let path = file
                    .path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return Err(too_large(format!("'{path}' is larger than 1 MiB")));
            }
            if size > self.remaining_bytes {
                return Err(too_large("the changes total more than 32 MiB".to_owned()));
            }
            self.remaining_bytes -= size;
        }
        Ok(())
    }
}

fn too_large(detail: String) -> ScanInputValidationError {
    ScanInputValidationError::RestrictedScanTooLarge { detail }
}

#[cfg(test)]
mod restricted_budget_tests {
    use super::*;

    fn temp_repo() -> (std::path::PathBuf, git2::Repository) {
        let dir =
            std::env::temp_dir().join(format!("stashbase-scan-budget-{}", uuid::Uuid::new_v4()));
        let repo = git2::Repository::init(&dir).unwrap();
        (dir, repo)
    }

    fn diff_adding<'repo>(
        repo: &'repo git2::Repository,
        files: &[(&str, usize)],
    ) -> git2::Diff<'repo> {
        let mut builder = repo.treebuilder(None).unwrap();
        for (name, size) in files {
            let blob = repo.blob(&vec![b'a'; *size]).unwrap();
            builder.insert(name, blob, 0o100644).unwrap();
        }
        let tree = repo.find_tree(builder.write().unwrap()).unwrap();
        repo.diff_tree_to_tree(None, Some(&tree), None).unwrap()
    }

    #[test]
    fn budget_accepts_ordinary_changes() {
        let (dir, repo) = temp_repo();
        let diff = diff_adding(&repo, &[("a.txt", 10), ("b.txt", 1024)]);

        assert!(RestrictedScanBudget::new()
            .charge_diff(&repo, &diff)
            .is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn budget_rejects_a_file_over_the_per_file_cap() {
        let (dir, repo) = temp_repo();
        let size = RESTRICTED_SCAN_MAX_FILE_BYTES as usize + 1;
        let diff = diff_adding(&repo, &[("big.txt", size)]);

        let error = RestrictedScanBudget::new()
            .charge_diff(&repo, &diff)
            .unwrap_err();

        assert!(
            matches!(&error, ScanInputValidationError::RestrictedScanTooLarge { detail } if detail.contains("big.txt")),
            "{error:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn budget_rejects_changes_over_the_total_cap_across_diffs() {
        let (dir, repo) = temp_repo();
        let per_file = RESTRICTED_SCAN_MAX_FILE_BYTES as usize;
        let files = (0..20)
            .map(|index| (format!("f{index}.txt"), per_file))
            .collect::<Vec<_>>();
        let files = files
            .iter()
            .map(|(name, size)| (name.as_str(), *size))
            .collect::<Vec<_>>();
        let (first, second) = files.split_at(16);
        let mut budget = RestrictedScanBudget::new();

        // 16 MiB and then 4 MiB fit; another 16 MiB, as in a later
        // commit's diff, crosses the 32 MiB total.
        assert!(budget
            .charge_diff(&repo, &diff_adding(&repo, first))
            .is_ok());
        assert!(budget
            .charge_diff(&repo, &diff_adding(&repo, second))
            .is_ok());
        let error = budget
            .charge_diff(&repo, &diff_adding(&repo, first))
            .unwrap_err();

        assert!(
            matches!(&error, ScanInputValidationError::RestrictedScanTooLarge { detail } if detail.contains("32 MiB")),
            "{error:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn budget_caps_the_number_of_commits() {
        let mut budget = RestrictedScanBudget::new();
        for _ in 0..RESTRICTED_SCAN_MAX_COMMITS {
            budget.charge_commit().unwrap();
        }

        assert!(matches!(
            budget.charge_commit(),
            Err(ScanInputValidationError::RestrictedScanTooLarge { .. })
        ));
    }
}
