use anyhow::Result;

use crate::models::api_client::{ApiPath, GetRequestApiResponse, RequestArgs};

use super::client;

pub struct ListArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub show_secrets: bool,
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse> {
    let ListArgs {
        token,
        project,
        environment,
        show_secrets,
    } = args;

    let mut query = vec![];

    if show_secrets == true {
        query.push(("secrets".to_string(), "true".to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: None,
        },
        query: Some(query),
        token,
    };

    client::get_request(args).await
}

pub struct GetArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse> {
    let GetArgs {
        token,
        project,
        environment,
        change_id,
    } = args;

    let mut query = vec![];

    // if descending == true {
    //     query.push(("descending".to_string(), "true".to_string()));
    // }

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: Some(change_id),
        },
        query: Some(query),
        token,
    };

    client::get_request(args).await
}
