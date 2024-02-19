use anyhow::Result;

use crate::models::api_client::{
    ApiPath, GetRequestApiResponse, PostPatchRequestApiResponse, RequestArgs,
};

use super::client;

pub struct ListArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse> {
    let ListArgs {
        api_key,
        project,
        environment,
    } = args;

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: None,
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub struct GetArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse> {
    let GetArgs {
        api_key,
        project,
        environment,
        webhook_id,
    } = args;

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: Some(webhook_id),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}
