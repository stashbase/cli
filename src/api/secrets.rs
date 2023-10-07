use anyhow::Result;

use super::client;
use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

pub async fn list(
    token: String,
    project: String,
    environment: String,
) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: None,
        token,
    };

    client::get_request(args).await
}
