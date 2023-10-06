use anyhow::Result;

use crate::models::{
    api_client::{ApiPath, GetRequestApiResponse, GetRequestArgs, PostRequestApiResponse},
    projects::CreateProjectPayload,
};

use super::client;

pub async fn list_projects(token: String) -> Result<GetRequestApiResponse> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    client::get_request(args).await
}

pub async fn create_project(
    token: String,
    data: CreateProjectPayload,
) -> Result<PostRequestApiResponse> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    client::post_request(args, Some(data)).await
}
