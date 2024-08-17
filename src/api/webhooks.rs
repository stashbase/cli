use anyhow::Result;

use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, RequestApiOptionResponse,
        RequestArgs,
    },
    webhooks::{CreateWebhookPayload, UpdateWebhookPayload, UpdateWebhookStatusPayload},
};

use super::client;

pub struct ListArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse> {
    let ListArgs {
        api_key,
        project,
        environment,
    } = args;

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: None,
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub struct GetArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub with_secret: bool,
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse> {
    let GetArgs {
        api_key,
        project,
        environment,
        webhook_id,
        with_secret,
    } = args;

    let query = match with_secret {
        true => Some(vec![("with-secret".to_string(), "true".to_string())]),
        false => None,
    };

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: Some(webhook_id),
        },
        query,
        api_key,
    };

    client::get_request(args).await
}

// create
pub struct CreateArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub return_secret: bool,
    pub data: CreateWebhookPayload,
}

pub async fn create(args: CreateArgs) -> Result<RequestApiOptionResponse> {
    let query = match args.return_secret {
        true => Some(vec![("return-secret".to_string(), "true".to_string())]),
        false => None,
    };

    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: None,
        },
        query,
        api_key: args.api_key,
    };

    client::post_request(req_args, Some(&args.data)).await
}

// update
pub struct TestArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
}

pub async fn test(args: TestArgs) -> Result<RequestApiOptionResponse> {
    let path = format!("{}/test", args.webhook_id);

    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: Some(path),
        },
        query: None,
        api_key: args.api_key,
    };

    client::post_request::<()>(req_args, None).await
}

// update
pub struct UpdateArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub data: UpdateWebhookPayload,
}

pub async fn update(args: UpdateArgs) -> Result<RequestApiOptionResponse> {
    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: Some(args.webhook_id),
        },
        query: None,
        api_key: args.api_key,
    };

    client::patch_request(req_args, Some(&args.data)).await
}

pub struct UpdateStatusArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub data: UpdateWebhookStatusPayload,
}

pub async fn update_status(args: UpdateStatusArgs) -> Result<RequestApiOptionResponse> {
    let path = format!("{}/status", args.webhook_id);

    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: Some(path),
        },
        query: None,
        api_key: args.api_key,
    };

    client::patch_request(req_args, Some(&args.data)).await
}

pub type GetSecretArgs = RotateArgs;

pub async fn get_secret(args: GetSecretArgs) -> Result<GetRequestApiResponse> {
    let path = format!("{}/signing-secret", args.webhook_id);

    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: Some(path),
        },
        query: None,
        api_key: args.api_key,
    };

    client::get_request(req_args).await
}

// update
pub struct RotateArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
}

pub async fn rotate_secret(args: RotateArgs) -> Result<RequestApiOptionResponse> {
    let path = format!("{}/secret", args.webhook_id);

    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: Some(path),
        },
        query: None,
        api_key: args.api_key,
    };

    client::post_request::<()>(req_args, None).await
}

// delete
pub struct DeleteArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
}

pub async fn delete(args: DeleteArgs) -> Result<DeleteRequestApiResponse> {
    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: Some(args.webhook_id),
        },
        query: None,
        api_key: args.api_key,
    };

    client::delete_request(req_args).await
}

// Logs
pub struct ListLogsArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub page: Option<usize>,
    // per page
    pub limit: Option<u8>,
}

pub async fn list_logs(args: ListLogsArgs) -> Result<GetRequestApiResponse> {
    let ListLogsArgs {
        api_key,
        project,
        environment,
        webhook_id,
        page,
        limit,
    } = args;

    let path = format!("{}/logs", webhook_id);

    let mut query = vec![];

    if let Some(page) = page {
        query.push(("page".to_string(), page.to_string()));
    }

    if let Some(limit) = limit {
        query.push(("limit".to_string(), limit.to_string()));
    }

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: Some(path),
        },
        query: if query.is_empty() { None } else { Some(query) },
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_dashboard_url(
    api_key: String,
    project: String,
    environment: String,
    webhook_id: &str,
) -> Result<GetRequestApiResponse> {
    let subpath = format!("{}/dashboard-url", webhook_id);

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: Some(subpath),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}
