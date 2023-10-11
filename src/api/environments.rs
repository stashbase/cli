use anyhow::Result;

use super::client;
use crate::{
    cmd::environments::{EnvSort, EnvironmentType},
    models::{
        api_client::{
            ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostPatchRequestApiResponse,
            RequestArgs,
        },
        environments::{
            CreatEnvironmentPayload, UpdateEnvironmentPayload, UpdateEnvironmentTypePayload,
        },
    },
};

pub struct ListEnvsRequestArgs {
    pub token: String,
    pub project: String,
    pub types: Vec<EnvironmentType>,
    pub locked: bool,
    pub unlocked: bool,
    pub sort: EnvSort,
    pub descending: bool,
}

pub async fn list(args: ListEnvsRequestArgs) -> Result<GetRequestApiResponse> {
    let ListEnvsRequestArgs {
        token,
        project,
        types,
        locked,
        unlocked,
        sort,
        descending,
    } = args;

    let mut query = vec![("sort".to_string(), format!("{}", sort))];

    if descending == true {
        query.push(("descending".to_string(), "true".to_string()));
    }

    if !types.is_empty() {
        let strings: Vec<_> = types.into_iter().map(|t| t.to_string()).collect();
        let joined = strings.join(",");

        query.push(("types".to_string(), joined));
    }

    if locked && !unlocked {
        query.push(("status".to_string(), "locked".to_string()));
    }

    if !locked && unlocked {
        query.push(("status".to_string(), "unlocked".to_string()));
    }

    let args = RequestArgs {
        token,
        query: Some(query),
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
    data: &CreatEnvironmentPayload,
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

pub async fn update(
    token: String,
    project: String,
    environment: String,
    data: &UpdateEnvironmentPayload,
) -> Result<PostPatchRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(environment),
        },
        query: None,
        token,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn update_type(
    token: String,
    project: String,
    environment: String,
    data: &UpdateEnvironmentTypePayload,
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
