use anyhow::Result;

use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostPatchRequestApiResponse,
        RequestArgs,
    },
    projects::{CreateProjectPayload, UpdateProjectPayload},
};

use super::client;

pub async fn list_projects(token: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    client::get_request(args).await
}

pub async fn get_project(token: String, name: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        token,
    };

    client::get_request(args).await
}

pub async fn get_project_url(token: String, name: String) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/link", name);

    let args = RequestArgs {
        path: ApiPath::Projects(Some(subpath)),
        token,
    };

    client::get_request(args).await
}

pub async fn create_project(
    token: String,
    data: CreateProjectPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_project(
    token: String,
    name: String,
    data: UpdateProjectPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        token,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn delete_project(token: String, name: String) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        token,
    };

    client::delete_request(args).await
}
