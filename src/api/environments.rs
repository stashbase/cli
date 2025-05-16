use super::client;
use crate::{
    cmd::environments::EnvSortBy,
    models::{
        api_client::{
            ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, OutputError,
            RequestApiOptionResponse, RequestArgs,
        },
        environments::{CreatEnvironmentPayload, UpdateEnvironmentPayload},
    },
};

pub struct ListEnvsRequestArgs {
    pub api_key: String,
    pub project: String,
    pub search: Option<String>,
    pub is_production: Option<bool>,
    pub sort_by: EnvSortBy,
    pub descending: bool,
}

pub async fn list(args: ListEnvsRequestArgs) -> Result<GetRequestApiResponse, OutputError> {
    let ListEnvsRequestArgs {
        api_key,
        project,
        search,
        is_production,
        sort_by: sort,
        descending,
    } = args;

    let mut query = vec![("sort-by".to_string(), format!("{}", sort))];

    if descending == true {
        query.push(("order".to_string(), "desc".to_string()));
    }

    if let Some(search) = search {
        query.push(("search".to_string(), search));
    }

    if let Some(is_production) = is_production {
        query.push(("is-production".to_string(), is_production.to_string()));
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
) -> Result<GetRequestApiResponse, OutputError> {
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

pub async fn get_url(
    api_key: String,
    project: String,
    identifier: String,
) -> Result<GetRequestApiResponse, OutputError> {
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
) -> Result<RequestApiOptionResponse, OutputError> {
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
) -> Result<RequestApiOptionResponse, OutputError> {
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

pub async fn delete(
    api_key: String,
    project: String,
    name: String,
) -> Result<DeleteRequestApiResponse, OutputError> {
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
    pub only_names: &'a bool,
}

pub async fn compare<'a>(
    args: CompareEnvironmentsRequestArgs<'a>,
) -> Result<GetRequestApiResponse, OutputError> {
    let CompareEnvironmentsRequestArgs {
        api_key,
        project,
        environment_1,
        environment_2,
        only_names,
    } = args;

    let path = format!("{}/compare/{}", environment_1, environment_2);

    let query = match only_names {
        true => Some(vec![(format!("hide-values"), format!("true"))]),
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
