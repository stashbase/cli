use crate::{
    cmd::{projects::SortBy, shared::Order},
    models::{
        api_client::{
            ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, OutputError,
            RequestApiOptionResponse, RequestArgs,
        },
        projects::{CreateProjectPayload, UpdateProjectPayload},
    },
};

use super::client;

pub async fn list_projects(
    api_key: String,
    search: Option<String>,
    sort_by: Option<SortBy>,
    order: Option<Order>,
    page: Option<usize>,
    page_size: Option<usize>,
) -> Result<GetRequestApiResponse, OutputError> {
    let mut query = Vec::new();

    if let Some(sort_by) = sort_by {
        query.push(("sort_by".to_string(), format!("{}", sort_by)));
    }

    if let Some(order) = order {
        query.push(("order".to_string(), order.to_string()));
    }

    if let Some(search) = search {
        query.push(("search".to_string(), search));
    }

    if let Some(page) = page {
        query.push(("page".to_string(), page.to_string()));
    }

    if let Some(page_size) = page_size {
        query.push(("page_size".to_string(), page_size.to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::Projects(None),
        query: Some(query),
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_project(
    api_key: String,
    identifier: String,
) -> Result<GetRequestApiResponse, OutputError> {
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
) -> Result<GetRequestApiResponse, OutputError> {
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
) -> Result<RequestApiOptionResponse, OutputError> {
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
) -> Result<RequestApiOptionResponse, OutputError> {
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
) -> Result<DeleteRequestApiResponse, OutputError> {
    let args = RequestArgs {
        path: ApiPath::Projects(Some(identifier)),
        query: None,
        api_key,
    };

    client::delete_request(args).await
}
