use anyhow::Result;

use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostRequestApiResponse,
        RequestArgs,
    },
    projects::CreateProjectPayload,
};

use super::client;

pub async fn list_projects(token: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    client::get_request(args).await
}

pub async fn create_project(
    token: String,
    data: CreateProjectPayload,
) -> Result<PostRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn delete_project(token: String, name: String) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        token,
    };

    client::delete_request(args).await
}
