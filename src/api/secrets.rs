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
    only_keys: bool,
) -> Result<GetRequestApiResponse> {
    let query = match only_keys {
        true => Some(vec![(format!("only-keys"), format!("true"))]),
        false => None,
    };

    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query,
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
