use anyhow::Result;

use crate::models::{
    api_client::{
        ApiPath, DeleteRequestApiResponse, GetRequestApiResponse, PostPatchRequestApiResponse,
        RequestArgs,
    },
    webhooks::{CreateWebhookPayload, UpdateWebhookPayload},
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
}

pub async fn get(args: GetArgs) -> Result<GetRequestApiResponse> {
    let GetArgs {
        api_key,
        project,
        environment,
        webhook_id,
    } = args;

    let args = RequestArgs {
        path: ApiPath::Webhooks {
            project,
            environment,
            path: Some(webhook_id),
        },
        query: None,
        api_key,
    };

    client::get_request(args).await
}

// create
pub struct CreateArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub data: CreateWebhookPayload,
}

pub async fn create(args: CreateArgs) -> Result<PostPatchRequestApiResponse> {
    let req_args = RequestArgs {
        path: ApiPath::Webhooks {
            project: args.project,
            environment: args.environment,
            path: None,
        },
        query: None,
        api_key: args.api_key,
    };

    client::post_request(req_args, Some(&args.data)).await
}

// update
pub struct UpdateArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub webhook_id: String,
    pub data: UpdateWebhookPayload,
}

pub async fn update(args: UpdateArgs) -> Result<PostPatchRequestApiResponse> {
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
