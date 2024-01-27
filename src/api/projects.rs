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
    api_key: String,
    search: Option<String>,
    sort: Sort,
    descending: bool,
) -> Result<GetRequestApiResponse> {
    let mut query = vec![("sort".to_string(), format!("{}", sort))];

    if descending == true {
        query.push(("descending".to_string(), "true".to_string()));
    }

    if let Some(search) = search {
        query.push(("search".to_string(), search));
    }

    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: Some(query),
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_project(api_key: String, name: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_project_url(api_key: String, name: String) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/url", name);

    let args = RequestArgs {
        path: ApiPath::Projects(Some(subpath)),
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub async fn create_project(
    api_key: String,
    data: &CreateProjectPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: None,
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_project(
    api_key: String,
    name: String,
    data: &UpdateProjectPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        query: None,
        api_key,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn delete_project(api_key: String, name: String) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(name)),
        query: None,
        api_key,
    };

    client::delete_request(args).await
}
