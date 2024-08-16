use anyhow::Result;

use super::client;
use crate::{
    cmd::environments::{EnvSort, EnvironmentType},
    models::{
        api_client::{
            ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, RequestApiOptionResponse,
            RequestArgs,
        },
        environments::{
            CreatEnvironmentPayload, DuplicateEnvironmentPayload, UpdateEnvironmentPayload,
        },
    },
};

pub struct ListEnvsRequestArgs {
    pub api_key: String,
    pub project: String,
    pub search: Option<String>,
    pub types: Vec<EnvironmentType>,
    pub locked: bool,
    pub unlocked: bool,
    pub sort: EnvSort,
    pub descending: bool,
}

pub async fn list(args: ListEnvsRequestArgs) -> Result<GetRequestApiResponse> {
    let ListEnvsRequestArgs {
        api_key,
        project,
        search,
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

    if let Some(search) = search {
        query.push(("search".to_string(), search));
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
        api_key,
        query: Some(query),
        path: ApiPath::Environments {
            project,
            path: None,
        },
    };

    client::get_request(args).await
}

pub async fn get(
    api_key: String,
    project: String,
    environment: String,
) -> Result<GetRequestApiResponse> {
    let args = RequestArgs {
        api_key,
        query: None,
        path: ApiPath::Environments {
            project,
            path: Some(environment),
        },
    };

    client::get_request(args).await
}

pub async fn load(
    api_key: String,
    project: String,
    environment: String,
    // data: &Option<LoadEnvironmentPayload>,
    only: Vec<String>,
    exclude: Vec<String>,
    expand_refs: bool,
) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/load", environment);

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

    let args = RequestArgs {
        api_key,
        query: Some(query),
        path: ApiPath::Environments {
            project,
            path: Some(subpath),
        },
    };

    client::get_request(args).await
}

pub async fn get_url(
    api_key: String,
    project: String,
    identifier: String,
) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/dashboard-url", identifier);

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(subpath),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub async fn create(
    api_key: String,
    project: String,
    open: bool,
    data: &CreatEnvironmentPayload,
) -> Result<RequestApiOptionResponse> {
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
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn update(
    api_key: String,
    project: String,
    environment: String,
    data: &UpdateEnvironmentPayload,
) -> Result<RequestApiOptionResponse> {
    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(environment),
        },
        query: None,
        api_key,
    };

    client::patch_request(args, Some(data)).await
}

pub async fn duplicate(
    api_key: String,
    project: String,
    environment: String,
    data: &DuplicateEnvironmentPayload,
) -> Result<RequestApiOptionResponse> {
    let path = format!("{}/duplicate", environment);

    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(path),
        },
        query: None,
        api_key,
    };

    client::post_request(args, Some(data)).await
}

pub async fn set_lock(
    api_key: String,
    project: String,
    environment: String,
    locked: bool,
) -> Result<RequestApiOptionResponse> {
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
        api_key,
    };

    client::patch_request::<()>(args, None).await
}

pub async fn delete(
    api_key: String,
    project: String,
    name: String,
) -> Result<DeleteRequestApiResponse> {
    let args = RequestArgs {
        path: ApiPath::Environments {
            project,
            path: Some(name),
        },
        query: None,
        api_key,
    };

    client::delete_request(args).await
}

pub struct CompareEnvironmentsRequestArgs<'a> {
    pub api_key: String,
    pub project: String,
    pub environment_1: &'a str,
    pub environment_2: &'a str,
    pub only_keys: &'a bool,
}

pub async fn compare<'a>(
    args: CompareEnvironmentsRequestArgs<'a>,
) -> Result<GetRequestApiResponse> {
    let CompareEnvironmentsRequestArgs {
        api_key,
        project,
        environment_1,
        environment_2,
        only_keys,
    } = args;

    let path = format!("{}/compare/{}", environment_1, environment_2);

    let query = match only_keys {
        true => Some(vec![(format!("only-keys"), format!("true"))]),
        false => None,
    };

    let args = RequestArgs {
        api_key,
        query,
        path: ApiPath::Environments {
            project,
            path: Some(path),
        },
    };

    client::get_request(args).await
}
