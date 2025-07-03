use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

use crate::utils::scans::should_merge_hunks;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    pub exclude: Option<Vec<String>>,

    #[serde(rename = "output-dir")]
    pub output_dir: Option<String>,

    #[serde(rename = "ignore-value-hashes")]
    pub ignore_value_hashes: Option<Vec<String>>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            exclude: None,
            output_dir: None,
            ignore_value_hashes: None,
        }
    }
}

impl ScanConfig {
    pub fn load_from_file(config_path: &str) -> Result<Self, anyhow::Error> {
        let file = std::fs::File::open(config_path)?;
        let config: ScanConfig = serde_yaml::from_reader(file)?;

        Ok(config)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRange {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRangeWithHash {
    pub start_line: usize,
    pub end_line: usize,
    pub content_hash: [u8; 32], //sha256 hash of the content
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub full_content: String,              // Hunks + context combined
    pub changes: Vec<ChangeRangeWithHash>, // Individual change ranges

    #[serde(rename = "startLine")]
    pub context_start_line: usize,

    #[serde(rename = "endLine")]
    pub context_end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHunks {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedFileHunksPayload {
    pub files: Vec<FileHunks>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_value_hashes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushCommitHunksPayload {
    pub commits: Vec<CommitChanges>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_value_hashes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitChanges {
    pub commit_id: String,
    pub files: Vec<FileHunks>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub file_path: String,
    pub range: ChangeRange,
    pub preview: String,
    pub severity: String,

    #[serde(rename = "valueSHA256")]
    pub value_sha256: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_id: Option<String>, // only for push commit hunks (pre-push hook)
}

impl Display for ScanResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!("File: {}\n", self.file_path));
        result.push_str(&format!(
            "Range: {},{}\n",
            self.range.start_line, self.range.end_line
        ));
        result.push_str(&format!("Preview: {}\n", self.preview));
        result.push_str(&format!("Severity: {}\n", self.severity));
        result.push_str(&format!("Value SHA256: {}", self.value_sha256));
        if let Some(id) = &self.commit_id {
            result.push_str(&format!("\nCommit: {}", id));
        }
        write!(f, "{}", result)
    }
}

impl ScanResult {
    pub fn get_colored_string(&self) -> String {
        let mut result = String::new();

        let (start_line, end_line) = (self.range.start_line, self.range.end_line);
        result.push_str(&format!("{} {}\n", "File:".green(), self.file_path));
        result.push_str(&format!(
            "{} {}-{}\n",
            "Range:".green(),
            start_line,
            end_line
        ));
        result.push_str(&format!("{} {}\n", "Preview:".green(), self.preview));
        result.push_str(&format!("{} {}\n", "Severity:".green(), self.severity));
        result.push_str(&format!(
            "{} {}\n",
            "Value SHA256:".green(),
            self.value_sha256
        ));
        if let Some(id) = &self.commit_id {
            result.push_str(&format!("{} {}\n", "Commit ID:".green(), id));
        }

        result
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StagedScanResponse {
    // if exceeded the limit of commits or files due to token limit, return the skipped commits or files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_files: Option<Vec<String>>,
    pub results: Vec<ScanResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommitScanResponse {
    // if exceeded the limit of commits or files due to token limit, return the skipped commits or files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_commits: Option<Vec<String>>,
    pub results: Vec<ScanResult>,
}

impl FileHunks {
    pub fn merge_overlapping_hunks(files: Vec<Self>, context_line_count: usize) -> Vec<Self> {
        // Group files by file path first
        let mut file_groups: std::collections::HashMap<String, Vec<DiffHunk>> =
            std::collections::HashMap::new();

        // Group hunks by file path
        for file in files {
            file_groups
                .entry(file.file_path.clone())
                .or_default()
                .extend(file.hunks);
        }

        // Process each file's hunks separately
        let mut result = Vec::new();
        for (file_path, hunks) in file_groups {
            if hunks.len() <= 1 {
                let file_hunks = Self { file_path, hunks };
                result.push(file_hunks);
                continue;
            }

            let mut sorted_hunks = hunks;
            sorted_hunks.sort_by_key(|h| h.context_start_line);

            let mut merged = Vec::new();
            let mut current = sorted_hunks[0].clone();

            for next in sorted_hunks.into_iter().skip(1) {
                if should_merge_hunks(&current, &next, context_line_count) {
                    // Extend the current hunk's context boundaries
                    current.context_end_line = current.context_end_line.max(next.context_end_line);

                    // Merge the full content intelligently to avoid duplication
                    let mut combined_lines: Vec<String> =
                        current.full_content.lines().map(String::from).collect();

                    let next_lines: Vec<String> = next
                        .full_content
                        .lines()
                        .map(String::from)
                        .skip_while(|line| combined_lines.contains(line))
                        .collect();

                    if !next_lines.is_empty() {
                        combined_lines.extend(next_lines);
                        current.full_content = combined_lines.join("\n");
                    }

                    // Combine the changes arrays
                    current.changes.extend(next.changes);

                    // Sort changes by start line and merge any that are consecutive
                    current.changes.sort_by_key(|change| change.start_line);
                    let mut merged_changes = Vec::new();
                    let current_change = current.changes.get(0).cloned();

                    if let Some(mut current_change) = current_change {
                        for next_change in current.changes.into_iter().skip(1) {
                            if next_change.start_line == current_change.end_line + 1 {
                                // Consecutive changes, merge them
                                current_change.end_line = next_change.end_line;
                            } else {
                                // Non-consecutive changes, push current and start new
                                merged_changes.push(current_change);
                                current_change = next_change;
                            }
                        }
                        merged_changes.push(current_change);
                    }
                    current.changes = merged_changes;
                } else {
                    merged.push(current);
                    current = next;
                }
            }
            merged.push(current);

            let result_file_hunks = Self {
                file_path,
                hunks: merged,
            };
            result.push(result_file_hunks);
        }

        result
    }
}
