use anyhow::Result;

use super::client;
use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostPatchRequestApiResponse,
        RequestArgs,
    },
    secrets::{
        DeleteSecretsPayload, GetSelectedSecretsPayload, RenameSecretsPayload, Secret,
        UpdateSecretDescriptionPayload,
    },
};

pub async fn list(
    token: String,
    project: String,
    environment: String,
    only_keys: bool,
) -> Result<GetRequestApiResponse> {
    let query = match only_keys {
        true => Some(vec![(format!("only-keys"), format!("true"))]),
        false => None,
    };

    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query,
        token,
    };

    client::get_request(args).await
}

// post with keys in body?
pub async fn get_selected(
    token: String,
    project: String,
    environment: String,
    data: &GetSelectedSecretsPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(String::from("selected")),
        },
        query: None,
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_description(
    token: String,
    project: String,
    environment: String,
    key: String,
    data: &UpdateSecretDescriptionPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(key),
        },
        query: None,
        token,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn delete(
    token: String,
    project: String,
    environment: String,
    data: &DeleteSecretsPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(String::from("delete")),
        },
        query: None,
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn delete_all(
    token: String,
    project: String,
    environment: String,
) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: None,
        token,
    };

    client::delete_request(args).await
}

pub async fn set_sercrets(
    token: String,
    project: String,
    environment: String,
    data: &Vec<Secret>,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: None,
        token,
    };

    client::post_request(args, Some(data)).await
}

pub async fn rename_secrets(
    token: String,
    project: String,
    environment: String,
    data: &RenameSecretsPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: None,
        token,
    };

    client::patch_request(args, Some(data)).await
}
