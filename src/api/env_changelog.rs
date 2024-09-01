use anyhow::Result;

use crate::models::api_client::{
    ApiPath, GetRequestApiResponse, RequestApiOptionResponse, RequestArgs,
};

use super::client;

pub struct ListArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub page: Option<usize>,
    pub limit: Option<usize>,
    pub show_values: bool,
    // pub show_secrets: bool,
    // pub only_secrets: bool,
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse> {
    let ListArgs {
        api_key,
        project,
        environment,
        show_values,
        page,
        limit,
    } = args;

    let mut query = vec![];

    if show_values == true {
        query.push(("with-values".to_string(), "true".to_string()));
    }

    if let Some(page) = page {
        query.push(("page".to_string(), page.to_string()));
    }

    if let Some(limit) = limit {
        query.push(("limit".to_string(), limit.to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: None,
        },
        query: Some(query),
        api_key,
    };

    client::get_request(args).await
}

pub struct GetArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse> {
    let GetArgs {
        api_key,
        project,
        environment,
        change_id,
    } = args;

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: Some(change_id),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub struct RevertArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
}

pub async fn revert(args: RevertArgs) -> Result<RequestApiOptionResponse> {
    let RevertArgs {
        api_key,
        project,
        environment,
        change_id,
    } = args;

    let path = format!("{}/revert", change_id);

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: Some(path),
        },
        query: None,
        api_key,
    };

    client::post_request::<()>(args, None).await
}
