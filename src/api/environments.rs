use anyhow::Result;

use super::client;
use crate::models::{
    api_client::{ApiPath, GetRequestApiResponse, PostPatchRequestApiResponse, RequestArgs},
    environments::CreatEnvironmentPayload,
};

pub async fn list(token: String, project: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        token,
        query: None,
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
        query: None,
        path: ApiPath::Environments {
            project,
            path: Some(environment),
        },
    };

    client::get_request(args).await
}

pub async fn get_url(
    token: String,
    project: String,
    environment: String,
) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/link", environment);

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(subpath),
        },
        query: None,
        token,
    };

    client::get_request(args).await
}

pub async fn create(
    token: String,
    project: String,
    open: bool,
    data: CreatEnvironmentPayload,
) -> Result<PostPatchRequestApiResponse> {
    let query = match open {
        true => Some(vec![(format!("url"), format!("true"))]),
        false => None,
    };

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: None,
        },
        query,
        token,
    };

    client::post_request(args, Some(data)).await
}
