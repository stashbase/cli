use std::env;

use anyhow::{bail, Context, Result};
use log::debug;
use reqwest::{header::HeaderMap, Method};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{
    default_on_request_failure, policies::ExponentialBackoff, RetryTransientMiddleware, Retryable,
    RetryableStrategy,
};

use crate::models::api_client::{
    ApiErrorResponse, CustomError, DeleteApiResponseOk, DeleteRequestApiResponse, GetApiResponseOk,
    GetRequestApiResponse, PostPatchApiResponseOk, PostPatchRequestApiResponse, RequestArgs,
};

struct RetryReqPolicy;
impl RetryableStrategy for RetryReqPolicy {
    fn handle(&self, res: &reqwest_middleware::Result<reqwest::Response>) -> Option<Retryable> {
        match res {
            // retry if 500
            Ok(success) if success.status() == 500 => Some(Retryable::Transient),
            // otherwise do not retry a successful request
            Ok(_) => None,
            // but maybe retry a request failure
            Err(error) => default_on_request_failure(error),
        }
    }
}

pub fn build_client(api_key: String) -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);

    // Create the actual middleware, with the exponential backoff and custom retry stategy.
    let ret_s =
        RetryTransientMiddleware::new_with_policy_and_strategy(retry_policy, RetryReqPolicy);

    let mut headers = HeaderMap::new();
    headers.insert("x-cli-key", api_key.parse().unwrap());

    let builder = ClientBuilder::new(
        reqwest::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(10))
            .default_headers(headers)
            .user_agent("env-ease-cli/0.0.1")
            .build()
            .unwrap(),
    )
    .with(ret_s);

    let client = builder.build();
    client
}

// pub fn get_request(args: GetRequestArgs) -> RequestBuilder {
//     let base_path =
//         env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));
//
//     let client = build_client();
//     let full_path = format!("{}/{}", base_path, args.path);
//
//     let mut headers = HeaderMap::new();
//     headers.insert("api_key", args.api_key.parse().unwrap());
//
//     client
//         .request(reqwest::Method::GET, full_path)
//         .headers(headers)
// }
//

pub async fn get_request(args: RequestArgs) -> Result<GetRequestApiResponse> {
    let base_path =
        env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));

    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", base_path, args.path);

    let res = client
        .request(reqwest::Method::GET, full_path)
        .query(&args.query)
        .send()
        .await;

    if let Err(_) = &res {
        bail!("Could not connect to API")
    }

    let res = res.unwrap();
    let status = res.status();

    if status.is_success() {
        let text = res.text().await.context("Could not parse response")?;
        let response = GetRequestApiResponse::Ok(GetApiResponseOk { status, text });

        Ok(response)
    } else {
        if status == 401 {
            let error = CustomError::unauthorized();
            Ok(GetRequestApiResponse::Err(error))
        } else if status == 429 {
            let error = CustomError::rate_limit_reached();
            Ok(GetRequestApiResponse::Err(error))
        } else {
            // TODO: return unknown error
            // this is for debugging
            let error_response: ApiErrorResponse = res
                .json()
                .await
                .with_context(|| "Failed to deserialize API error response")?;

            // Convert the API error into your custom error type
            let custom_error: CustomError = error_response.error.into();
            Ok(GetRequestApiResponse::Err(custom_error))
        }
    }
}

pub async fn delete_request(args: RequestArgs) -> Result<DeleteRequestApiResponse> {
    let base_path =
        env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));

    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", base_path, args.path);

    let res = client
        .request(reqwest::Method::DELETE, full_path)
        .query(&args.query)
        .send()
        .await;

    if let Err(_) = &res {
        bail!("Could not connect to API")
    }

    let res = res.unwrap();
    let status = res.status();

    if status.is_success() {
        if let Some(content_length) = res.content_length() {
            if (content_length as usize) == 0 {
                let response =
                    DeleteRequestApiResponse::Ok(DeleteApiResponseOk { status, text: None });

                Ok(response)
            } else {
                let text = res.text().await.context("Could not parse response")?;
                let response = DeleteRequestApiResponse::Ok(DeleteApiResponseOk {
                    status,
                    text: Some(text),
                });

                Ok(response)
            }
        } else {
            Ok(DeleteRequestApiResponse::Ok(DeleteApiResponseOk {
                status,
                text: None,
            }))
        }
    } else {
        if status == 401 {
            let error = CustomError::unauthorized();
            Ok(DeleteRequestApiResponse::Err(error))
        } else if status == 404 {
            let error = CustomError::unknown();
            Ok(DeleteRequestApiResponse::Err(error))
        } else if status == 429 {
            let error = CustomError::rate_limit_reached();
            Ok(DeleteRequestApiResponse::Err(error))
        } else {
            let error_response: ApiErrorResponse = res
                .json()
                .await
                .with_context(|| "Failed to deserialize API error response")?;

            // Convert the API error into your custom error type
            let custom_error: CustomError = error_response.error.into();
            Ok(DeleteRequestApiResponse::Err(custom_error))
        }
    }
}

pub async fn post_request<T>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<PostPatchRequestApiResponse>
where
    T: serde::Serialize,
{
    post_or_pach(args, Some(data), reqwest::Method::POST).await
}

pub async fn patch_request<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<PostPatchRequestApiResponse> {
    post_or_pach(args, data, reqwest::Method::PATCH).await
}

async fn post_or_pach<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<T>,
    method: Method,
) -> Result<PostPatchRequestApiResponse> {
    let base_path =
        env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));

    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", base_path, args.path);

    let mut headers = HeaderMap::new();

    debug!("Query: {:#?}", args.query);

    let res = match data {
        Some(data) => {
            headers.insert("Content-Type", "application/json".parse().unwrap());

            client
                .request(method, full_path)
                .headers(headers)
                .query(&args.query)
                .json(&data)
                .send()
                .await
        }
        None => {
            client
                .request(method, full_path)
                .headers(headers)
                .query(&args.query)
                .send()
                .await
        }
    };

    if let Err(_) = &res {
        bail!("Could not connect to API")
    }

    let res = res.unwrap();
    let status = res.status();

    if status.is_success() {
        if let Some(content_length) = res.content_length() {
            if (content_length as usize) == 0 {
                let response =
                    PostPatchRequestApiResponse::Ok(PostPatchApiResponseOk { status, text: None });

                Ok(response)
            } else {
                let text = res.text().await.context("Could not parse response")?;
                let response = PostPatchRequestApiResponse::Ok(PostPatchApiResponseOk {
                    status,
                    text: Some(text),
                });

                Ok(response)
            }
        } else {
            let response =
                PostPatchRequestApiResponse::Ok(PostPatchApiResponseOk { status, text: None });

            Ok(response)
        }
    } else {
        if status == 401 {
            let error = CustomError::unauthorized();
            Ok(PostPatchRequestApiResponse::Err(error))
        } else if status == 429 {
            let error = CustomError::rate_limit_reached();
            Ok(PostPatchRequestApiResponse::Err(error))
        } else {
            let error_response: ApiErrorResponse = res
                .json()
                .await
                .with_context(|| "Failed to deserialize API error response")?;

            // Convert the API error into your custom error type
            let custom_error: CustomError = error_response.error.into();
            Ok(PostPatchRequestApiResponse::Err(custom_error))
        }
    }
}

// pub fn post_request<T>(args: GetRequestArgs, data: Option<T>) -> Result
// where
//     T: serde::Serialize,
// {
//     let base_path =
//         env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));
//
//     let client = build_client();
//     let full_path = format!("{}/{}", base_path, args.path);
//
//     let mut headers = HeaderMap::new();
//     headers.insert("api_key", args.api_key.parse().unwrap());
//
//     match data {
//         Some(data) => {
//             headers.insert("Content-Type", "application/json".parse().unwrap());
//
//             client
//                 .request(reqwest::Method::POST, full_path)
//                 .headers(headers)
//                 .json(&data)
//         }
//         None => client
//             .request(reqwest::Method::POST, full_path)
//             .headers(headers),
//     }
// }
