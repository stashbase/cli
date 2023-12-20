use anyhow::Result;

use crate::models::api_client::{
    ApiPath, GetRequestApiResponse, PostPatchRequestApiResponse, RequestArgs,
};

use super::client;

pub struct ListArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub page: Option<usize>,
    pub show_values: bool,
    // pub show_secrets: bool,
    // pub only_secrets: bool,
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse> {
    let ListArgs {
        token,
        project,
        environment,
        show_values,
        page,
    } = args;

    let mut query = vec![];

    if show_values == true {
        query.push(("hidden".to_string(), "false".to_string()));
    }

    // if only_secrets == true {
    //     query.push(("only-secrets".to_string(), "true".to_string()));
    // }
    //
    if let Some(page) = page {
        query.push(("page".to_string(), page.to_string()));
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

    let args = RequestArgs {
        path: ApiPath::EnvChangelog {
            project,
            environment,
            path: Some(change_id),
        },
        query: None,
        token,
    };

    client::get_request(args).await
}

pub struct RevertArgs {
    pub token: String,
    pub project: String,
    pub environment: String,
    pub change_id: String,
}

pub async fn revert(args: RevertArgs) -> Result<PostPatchRequestApiResponse> {
    let RevertArgs {
        token,
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
        token,
    };

    client::post_request::<()>(args, None).await
}
