use super::client;
use crate::models::{
    api_client::{ApiPath, OutputError, RequestApiOptionResponse, RequestArgs},
    scans::{PushCommitHunksPayload, StagedFileHunksPayload},
};

pub async fn scan_staged_hunks(
    api_key: String,
    data: &StagedFileHunksPayload,
) -> Result<RequestApiOptionResponse, OutputError> {
    let args = RequestArgs {
        path: ApiPath::Scan {
            path: "hunks".to_string(),
        },
        query: None,
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn scan_commits(
    api_key: String,
    data: &PushCommitHunksPayload,
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
