use anyhow::Result;

use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

use super::client;

pub struct ListArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse> {
    let ListArgs {
        token,
        project,
        environment,
    } = args;

    let mut query = vec![];

    // if descending == true {
    //     query.push(("descending".to_string(), "true".to_string()));
    // }

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
        },
        query: Some(query),
        token,
    };

    client::get_request(args).await
}
