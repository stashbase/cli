use super::client;
use crate::models::api_client::{ApiPath, GetRequestApiResponse, OutputError, RequestArgs};

pub async fn get_url(api_key: String) -> Result<GetRequestApiResponse, OutputError> {
    let subpath = format!("dashboard-url");

    let args = RequestArgs {
        path: ApiPath::Workspace {
            path: Some(subpath),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}
