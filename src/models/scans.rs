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

// staged file wiht hunk content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedFileHunks {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
}
