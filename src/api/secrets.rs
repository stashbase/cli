use anyhow::Result;

use super::client;
use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, RequestApiOptionResponse,
        RequestArgs,
    },
    secrets::{RenameSecretsPayload, Secret, UpdateSecretDescriptionPayload},
};

pub async fn list(
    api_key: String,
    project: String,
    environment: String,
    only_names: bool,
    only: Option<Vec<String>>,
    expand_refs: bool,
) -> Result<GetRequestApiResponse> {
    let mut query_str = vec![];

    if only_names {
        query_str.push(("omit".to_string(), "value,description".to_string()));
    } else {
        query_str.push(("expand-refs".to_string(), expand_refs.to_string()));
    }

    if let Some(only) = only {
        query_str.push(("only".to_string(), only.join(",")));
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
    with_description: bool,
    expand_refs: bool,
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

    query.push(("expand-refs".to_string(), expand_refs.to_string()));

    if with_description == false {
        query.push(("omit".to_string(), "description".to_string()));
    }

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

pub async fn update_description(
    api_key: String,
    project: String,
    environment: String,
    name: String,
    data: &UpdateSecretDescriptionPayload,
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(format!("{}", name)),
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
    secrets_to_delete: &Vec<String>,
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(String::from("delete")),
        },
        query: None,
        api_key,
    };

    client::post_request(args, Some(secrets_to_delete)).await
}

pub async fn delete_all(
    api_key: String,
    project: String,
    environment: String,
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: Some(String::from("delete/all")),
        },
        query: None,
        api_key,
    };

    client::post_request::<()>(args, None).await
}

pub async fn set_sercrets(
    api_key: String,
    project: String,
    environment: String,
    data: &Vec<Secret>,
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Secrets {
            project,
            environment,
            path: None,
        },
        query: None,
        api_key,
    };

    client::put_request(args, Some(data)).await
}

pub async fn rename_secrets(
    api_key: String,
    project: String,
    environment: String,
    data: &RenameSecretsPayload,
) -> Result<RequestApiOptionResponse> {
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
