use sha2::{Sha256, Digest};
use std::{path::Path, fs, collections::HashSet};
use ignore::gitignore::GitignoreBuilder;
use crate::models::{
    scans::{DiffHunk, FileChangesScanResponse, ScanFinding, ChangeRangeWithHash},
    validation::ScanInputValidationError,
};
use std::time::{SystemTime, UNIX_EPOCH};
use git2;

pub static SCAN_IGNORE_LINE_COMMENT: &str = "@stashbase-ignore";
pub static SCAN_CONTEXT_LINES: usize = 10;

pub fn should_merge_hunks(hunk1: &DiffHunk, hunk2: &DiffHunk, max_gap: usize) -> bool {
    // Only merge if they're close enough
    if (hunk2.context_start_line as i64 - hunk1.context_end_line as i64).abs() > max_gap as i64 {
        return false;
    }

    // Check for context overlap
    hunk1.context_end_line >= hunk2.context_start_line
        || (hunk2.context_start_line - hunk1.context_end_line) <= max_gap
}

pub fn get_comment_prefix(extension: &str) -> Option<&'static str> {
    match extension {
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
    match extension {
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

pub fn should_exclude_file(file_path: &str, exclude_patterns: &[String]) -> bool {
    let mut builder = GitignoreBuilder::new("/"); // Root directory
    for pattern in exclude_patterns {
        builder.add_line(None, pattern).unwrap();
    }
    let gitignore = builder.build().unwrap();
    gitignore.matched(Path::new(file_path), false).is_ignore()
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

pub fn save_scan_results(output_dir: &str, json_content: &str) -> String {
    // Create scan_results directory if it doesn't exist
    fs::create_dir_all(output_dir).unwrap();

    // Get current timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Create file path
    let file_path = format!("{}/{}.json", output_dir, timestamp);

    // Write to file
    fs::write(&file_path, json_content).unwrap();

    file_path
}

pub fn filter_sha256_hashes(hashes: Vec<String>) -> Vec<String> {
    hashes
        .into_iter()
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .collect::<Vec<_>>()
}

pub fn load_baseline_results(baseline_path: &str) -> Result<Vec<ScanFinding>, ScanInputValidationError> {
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

    let baseline_response: FileChangesScanResponse = serde_json::from_str(&content).map_err(|e| {
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
    hasher.update(finding.range.start_line.to_string().as_bytes());
    hasher.update(finding.range.end_line.to_string().as_bytes());
    hasher.update(finding.value_sha256.as_bytes());
    hasher.update(finding.preview.as_bytes());
    hasher.update(finding.severity.to_string().as_bytes());
    if let Some(commit_id) = &finding.commit_id {
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
        .filter(|finding| {
            !baseline_hashes.contains(&compute_finding_hash(finding))
        })
        .collect::<Vec<_>>();

    let mut sorted_findings: Vec<_> = filtered_findings.into_iter().collect();

    sorted_findings.sort_by(|a, b| {
        (b.severity.clone() as i32).cmp(&(a.severity.clone() as i32)) // by severity, descending
            .then(a.file_path.cmp(&b.file_path))      // then by file path
            .then(a.range.start_line.cmp(&b.range.start_line)) // then by start line
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
        last_hunk.context_end_line = line_number;
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
            let line_without_comment_prefix = content
                .trim()
                .trim_start_matches(comment_prefix)
                .trim();

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
                                .map(|changes| changes.iter().any(|change| change.content_hash == content_hash))
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
                            .map(|changes| changes.iter().any(|change| change.content_hash == content_hash))
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

            let line_without_comment_prefix = content.trim().trim_start_matches(comment_prefix).trim();

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
                            .map(|changes| changes.iter().any(|change| change.content_hash == content_hash))
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
                            .map(|changes| changes.iter().any(|change| change.content_hash == content_hash))
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