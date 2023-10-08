use anyhow::Result;

use super::client;
use crate::models::{
    api_client::{ApiPath, GetRequestApiResponse, PostPatchRequestApiResponse, RequestArgs},
    secrets::GetSelectedSecretsPayload,
};

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

// post with keys in body?
pub async fn get_selected(
    token: String,
    project: String,
    environment: String,
    data: GetSelectedSecretsPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(String::from("selected")),
        },
        query: None,
        token,
    };

    client::post_request(args, Some(data)).await
}
