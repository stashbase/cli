use anyhow::Result;

use super::client;
use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

pub async fn list(token: String, project: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        token,
        path: ApiPath::Environments {
            project,
            path: None,
        },
    };

    client::get_request(args).await
}

pub async fn get(
    token: String,
    project: String,
    environment: String,
) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        token,
        path: ApiPath::Environments {
            project,
            path: Some(environment),
        },
    };

    client::get_request(args).await
}
