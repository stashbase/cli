use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRange {
    pub start_line: usize,
    pub end_line: usize,

    #[serde(skip)]
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub full_content: String,      // Hunks + context combined
    pub changes: Vec<ChangeRange>, // Individual change ranges

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
pub struct StagedFileHunksPayload {
    pub files: Vec<FileHunks>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushCommitHunksPayload {
    pub commits: Vec<CommitHunks>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_value_hashes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitHunks {
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
