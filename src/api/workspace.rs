use anyhow::Result;

use super::client;
use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

pub async fn get_url(token: String) -> Result<GetRequestApiResponse> {
    let subpath = format!("url");

    let args = RequestArgs {
        path: ApiPath::Workspace {
            path: Some(subpath),
        },
        query: None,
        token,
    };

    client::get_request(args).await
}
