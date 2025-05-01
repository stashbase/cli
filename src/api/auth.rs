use anyhow::Result;

use super::client;

use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

// for whoami command
pub struct GetCurrentAuthDetailsRequestArgs {
    pub api_key: String,
}

pub async fn get_current_auth_details(
    args: GetCurrentAuthDetailsRequestArgs,
) -> Result<GetRequestApiResponse> {
    let args = args;

    let response = client::get_request(RequestArgs {
        api_key: args.api_key,
        path: ApiPath::Whoami,
        query: None,
    })
    .await?;

    Ok(response)
}
