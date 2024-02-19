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
    pub change_id: String,
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse> {
    let GetArgs {
        api_key,
        project,
        environment,
        change_id,
    } = args;

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: Some(change_id),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub struct RevertArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
}

pub async fn revert(args: RevertArgs) -> Result<PostPatchRequestApiResponse> {
    let RevertArgs {
        api_key,
        project,
        environment,
        change_id,
    } = args;

    let path = format!("{}/revert", change_id);

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: Some(path),
        },
        query: None,
        api_key,
    };

    client::post_request::<()>(args, None).await
}
