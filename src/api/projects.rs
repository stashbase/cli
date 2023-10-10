use anyhow::Result;

use crate::{
    cmd::projects::Sort,
    models::{
        api_client::{
            ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostPatchRequestApiResponse,
            RequestArgs,
        },
        projects::{CreateProjectPayload, UpdateProjectPayload},
    },
};

use super::client;

pub async fn list_projects(
    token: String,
    sort: Sort,
    descending: bool,
) -> Result<GetRequestApiResponse> {
    let mut query = vec![("sort".to_string(), format!("{}", sort))];

    if descending == true {
        query.push(("descending".to_string(), "true".to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: Some(query),
        token,
    };

    client::get_request(args).await
}

pub async fn get_project(token: String, name: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        query: None,
        token,
    };

    client::get_request(args).await
}

pub async fn get_project_url(token: String, name: String) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/link", name);

    let args = RequestArgs {
        path: ApiPath::Projects(Some(subpath)),
        query: None,
        token,
    };

    client::get_request(args).await
}

pub async fn create_project(
    token: String,
    data: &CreateProjectPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: None,
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_project(
    token: String,
    name: String,
    data: &UpdateProjectPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        query: None,
        token,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn delete_project(token: String, name: String) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        query: None,
        token,
    };

    client::delete_request(args).await
}
