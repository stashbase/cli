use reqwest::{Error, Response};

use crate::models::{
    api_client::{ApiPath, GetRequestArgs},
    projects::CreateProjectPayload,
};

use super::client;

pub async fn list_projects(token: String) -> Result<Response, Error> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    let request = client::get_request(args);

    return request.send().await;
}

pub async fn create_project(token: String, data: CreateProjectPayload) -> Result<Response, Error> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    let request = client::post_request(args, Some(data));

    return request.send().await;
}
