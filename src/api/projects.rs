use reqwest::{Error, Response};

use crate::models::{
    api_client::{ApiPath, GetRequestApiResponse, GetRequestArgs},
    projects::CreateProjectPayload,
};

use super::client;

pub async fn list_projects(token: String) -> anyhow::Result<GetRequestApiResponse> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    let response = client::get_request(args).await;

    return response;
}

pub async fn create_project(token: String, data: CreateProjectPayload) -> Result<Response, Error> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    let request = client::post_request(args, Some(data));

    return request.send().await;
}
