use anyhow::Result;

use super::client;
use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

pub async fn get_url(api_key: String) -> Result<GetRequestApiResponse> {
    let subpath = format!("dashboard-url");

    let args = RequestArgs {
        path: ApiPath::Workspace {
            path: Some(subpath),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}
