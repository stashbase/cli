use reqwest::{Error, Response};

use crate::models::api_client::{ApiPath, GetRequestArgs};

use super::client;

pub async fn list_projects(token: String) -> Result<Response, Error> {
    let args = GetRequestArgs {
        path: ApiPath::Projects(None),
        token,
    };

    let request = client::get_request(args);

    return request.send().await;
}
