use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, OutputError,
        RequestApiOptionResponse, RequestArgs,
    },
    webhooks::{CreateWebhookPayload, UpdateWebhookPayload, UpdateWebhookStatusPayload},
};

use super::client;

pub struct ListArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
}

fn get_webhooks_path(
    project: Option<String>,
    environment: Option<String>,
    path: Option<String>,
) -> ApiPath {
    match (project, environment) {
        (Some(project), Some(environment)) => ApiPath::Webhooks {
            project,
            environment,
            path,
        },
        _ => ApiPath::WebhooksEnvScope { path },
    }
}

pub async fn list(args: ListArgs) -> Result<GetRequestApiResponse, OutputError> {
    let ListArgs {
        api_key,
        project,
        environment,
    } = args;

    let path = get_webhooks_path(project, environment, None);

    let args = RequestArgs {
        path,
        query: None,
        api_key,
    };

    client::get_request(args).await
}

pub struct GetArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
    pub include_secret: bool,
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse, OutputError> {
    let GetArgs {
        api_key,
        project,
        environment,
        webhook_id,
        include_secret,
    } = args;

    let query = match include_secret {
        true => Some(vec![("include_secret".to_string(), "true".to_string())]),
        false => None,
    };

    let path = get_webhooks_path(project, environment, Some(webhook_id));

    let args = RequestArgs {
        path,
        query,
        api_key,
    };

    client::get_request(args).await
}

// create
pub struct CreateArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub data: CreateWebhookPayload,
}

pub async fn create(args: CreateArgs) -> Result<RequestApiOptionResponse, OutputError> {
    let path = get_webhooks_path(args.project, args.environment, None);

    let req_args = RequestArgs {
        path,
        query: None,
        api_key: args.api_key,
    };

    client::post_request(req_args, Some(&args.data)).await
}

// update
pub struct TestArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
}

pub async fn test(args: TestArgs) -> Result<RequestApiOptionResponse, OutputError> {
    let path = get_webhooks_path(
        args.project,
        args.environment,
        Some(format!("{}/test", args.webhook_id)),
    );

    let req_args = RequestArgs {
        path,
        query: None,
        api_key: args.api_key,
    };

    client::post_request::<()>(req_args, None).await
}

// update
pub struct UpdateArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
    pub data: UpdateWebhookPayload,
}

pub async fn update(args: UpdateArgs) -> Result<RequestApiOptionResponse, OutputError> {
    let path = get_webhooks_path(args.project, args.environment, Some(args.webhook_id));

    let req_args = RequestArgs {
        path,
        query: None,
        api_key: args.api_key,
    };

    client::patch_request(req_args, Some(&args.data)).await
}

pub struct UpdateStatusArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
    pub data: UpdateWebhookStatusPayload,
}

pub async fn update_status(
    args: UpdateStatusArgs,
) -> Result<RequestApiOptionResponse, OutputError> {
    let path = get_webhooks_path(
        args.project,
        args.environment,
        Some(format!("{}/status", args.webhook_id)),
    );

    let req_args = RequestArgs {
        path,
        query: None,
        api_key: args.api_key,
    };

    client::patch_request(req_args, Some(&args.data)).await
}

pub type GetSecretArgs = RotateArgs;

pub async fn get_secret(args: GetSecretArgs) -> Result<GetRequestApiResponse, OutputError> {
    let path = get_webhooks_path(
        args.project,
        args.environment,
        Some(format!("{}/signing-secret", args.webhook_id)),
    );

    let req_args = RequestArgs {
        path,
        query: None,
        api_key: args.api_key,
    };

    client::get_request(req_args).await
}

// update
pub struct RotateArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
}

pub async fn rotate_secret(args: RotateArgs) -> Result<RequestApiOptionResponse, OutputError> {
    let path = get_webhooks_path(
        args.project,
        args.environment,
        Some(format!("{}/signing-secret", args.webhook_id)),
    );

    let req_args = RequestArgs {
        path,
        query: None,
        api_key: args.api_key,
    };

    client::post_request::<()>(req_args, None).await
}

// delete
pub struct DeleteArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
}

pub async fn delete(args: DeleteArgs) -> Result<DeleteRequestApiResponse, OutputError> {
    let req_args = RequestArgs {
        path: get_webhooks_path(args.project, args.environment, Some(args.webhook_id)),
        query: None,
        api_key: args.api_key,
    };

    client::delete_request(req_args).await
}

// Logs
pub struct ListLogsArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub webhook_id: String,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}

pub async fn list_logs(args: ListLogsArgs) -> Result<GetRequestApiResponse, OutputError> {
    let ListLogsArgs {
        api_key,
        project,
        environment,
        webhook_id,
        page,
        page_size,
    } = args;

    let path = get_webhooks_path(project, environment, Some(format!("{}/logs", webhook_id)));

    let mut query = vec![];

    if let Some(page) = page {
        query.push(("page".to_string(), page.to_string()));
    }

    if let Some(page_size) = page_size {
        query.push(("page_size".to_string(), page_size.to_string()));
    }

    let args = RequestArgs {
        path,
        query: if query.is_empty() { None } else { Some(query) },
        api_key,
    };

    client::get_request(args).await
}

pub async fn get_dashboard_url(
    api_key: String,
    project: Option<String>,
    environment: Option<String>,
    webhook_id: &str,
) -> Result<GetRequestApiResponse, OutputError> {
    let subpath = format!("{}/dashboard-url", webhook_id);

    let path = get_webhooks_path(project, environment, Some(subpath));
    let args = RequestArgs {
        path,
        query: None,
        api_key,
    };

    client::get_request(args).await
}
