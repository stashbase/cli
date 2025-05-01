use anyhow::Result;

use super::client;

use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

// for whoami command
pub async fn get_current_auth_details(api_key: String) -> Result<GetRequestApiResponse> {
    let response = client::get_request(RequestArgs {
        api_key,
        path: ApiPath::Whoami,
        query: None,
    })
    .await?;

    Ok(response)
}
