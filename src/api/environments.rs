use anyhow::Result;

use super::client;
use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostPatchRequestApiResponse,
        RequestArgs,
    },
    environments::{CreatEnvironmentPayload, UpdateEnvironmentTypePayload},
};

pub async fn list(token: String, project: String) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        token,
        query: None,
        path: ApiPath::Environments {
            project,
            path: None,
        },
    };

    client::get_request(args).await
}

pub async fn get(
    token: String,
    project: String,
    environment: String,
) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        token,
        query: None,
        path: ApiPath::Environments {
            project,
            path: Some(environment),
        },
    };

    client::get_request(args).await
}

pub async fn get_url(
    token: String,
    project: String,
    environment: String,
) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/link", environment);

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(subpath),
        },
        query: None,
        token,
    };

    client::get_request(args).await
}

pub async fn create(
    token: String,
    project: String,
    open: bool,
    data: CreatEnvironmentPayload,
) -> Result<PostPatchRequestApiResponse> {
    let query = match open {
        true => Some(vec![(format!("url"), format!("true"))]),
        false => None,
    };

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: None,
        },
        query,
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_type(
    token: String,
    project: String,
    environment: String,
    data: UpdateEnvironmentTypePayload,
) -> Result<PostPatchRequestApiResponse> {
    let subpath = format!("{}/type", environment);

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(subpath),
        },
        query: None,
        token,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn set_lock(
    token: String,
    project: String,
    environment: String,
    locked: bool,
) -> Result<PostPatchRequestApiResponse> {
    let subpath = match locked {
        true => format!("{}/lock", environment),
        false => format!("{}/unlock", environment),
    };

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(subpath),
        },
        query: None,
        token,
    };

    client::patch_request::<()>(args, None).await
}

pub async fn delete(
    token: String,
    project: String,
    name: String,
) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(name),
        },
        query: None,
        token,
    };

    client::delete_request(args).await
}
