use anyhow::Result;

use crate::{
    cmd::projects::SortBy,
    models::{
        api_client::{
            ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, RequestApiOptionResponse,
            RequestArgs,
        },
        projects::{CreateProjectPayload, UpdateProjectPayload},
    },
};

use super::client;

pub async fn list_projects(
    api_key: String,
    search: Option<String>,
    sort_by: SortBy,
    descending: bool,
    page: Option<usize>,
    limit: Option<usize>,
) -> Result<GetRequestApiResponse> {
    let mut query = vec![("sort-by".to_string(), format!("{}", sort_by))];

    if descending == true {
        query.push(("order".to_string(), "desc".to_string()));
    }

    if let Some(search) = search {
        query.push(("search".to_string(), search));
    }

    if let Some(page) = page {
        query.push(("page".to_string(), page.to_string()));
    }

    if let Some(limit) = limit {
        query.push(("limit".to_string(), limit.to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: Some(query),
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_project(api_key: String, identifier: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(identifier)),
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_project_dashboard_url(
    api_key: String,
    identifier: String,
) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/dashboard-url", identifier);

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
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: None,
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_project(
    api_key: String,
    identifier: String,
    data: &UpdateProjectPayload,
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(identifier)),
        query: None,
        api_key,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn delete_project(
    api_key: String,
    identifier: String,
) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(identifier)),
        query: None,
        api_key,
    };

    client::delete_request(args).await
}
