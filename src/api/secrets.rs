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
    api_key: String,
    project: String,
    environment: String,
    only_keys: bool,
    resolve_refs: bool,
) -> Result<GetRequestApiResponse> {
    let mut query_str = vec![];

    if only_keys {
        query_str.push(("only-keys".to_string(), "true".to_string()));
    } else {
        query_str.push(("resolve-refs".to_string(), resolve_refs.to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: match !query_str.is_empty() {
            true => Some(query_str),
            false => None,
        },
        api_key,
    };

    client::get_request(args).await
}

pub async fn pull(
    api_key: String,
    project: String,
    environment: String,
    only: Vec<String>,
    exclude: Vec<String>,
    resolve_refs: bool,
) -> Result<GetRequestApiResponse> {
    let mut query = match !only.is_empty() || !exclude.is_empty() {
        true => {
            let mut query = vec![];

            if !only.is_empty() {
                query.push(("only".to_string(), only.join(",")));
            }

            if !exclude.is_empty() {
                query.push(("exclude".to_string(), exclude.join(",")));
            }

            query
        }
        false => Vec::with_capacity(1),
    };

    query.push(("resolve-refs".to_string(), resolve_refs.to_string()));

    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: Some(query),
        api_key,
    };

    client::get_request(args).await
}

// post with keys in body?
pub async fn get_selected(
    api_key: String,
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
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update_description(
    api_key: String,
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
        api_key,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn delete(
    api_key: String,
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
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn delete_all(
    api_key: String,
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
        api_key,
    };

    client::delete_request(args).await
}

pub async fn set_sercrets(
    api_key: String,
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
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn rename_secrets(
    api_key: String,
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
        api_key,
    };

    client::patch_request(args, Some(data)).await
}
