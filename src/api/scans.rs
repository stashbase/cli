use super::client;
use crate::models::{
    api_client::{ApiPath, OutputError, RequestApiOptionResponse, RequestArgs},
    scans::{ScanCommitChangesPayload, ScanFileChangesPayload},
};

pub async fn scan_file_changes(
    api_key: String,
    data: &ScanFileChangesPayload,
) -> Result<RequestApiOptionResponse, OutputError> {
    let args = RequestArgs {
        path: ApiPath::Scan {
            path: "file-changes".to_string(),
        },
        query: None,
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn scan_commits(
    api_key: String,
    data: &ScanCommitChangesPayload,
) -> Result<RequestApiOptionResponse, OutputError> {
    let args = RequestArgs {
        path: ApiPath::Scan {
            path: "commits".to_string(),
        },
        query: None,
        api_key,
    };

    client::post_request(args, Some(data)).await
}
