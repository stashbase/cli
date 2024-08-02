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
    GetRequestApiResponse, OptionResponseOk, RequestApiOptionResponse, RequestArgs,
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
    headers.insert("x-api-key", api_key.parse().unwrap());

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
    let base_path = env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:5000"));

    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", base_path, args.path);

    let res = client
        .request(reqwest::Method::GET, full_path)
        .query(&args.query)
        .send()
        .await;

    if let Err(_) = &res {
        let err = CustomError::cannot_connect();
        bail!(err)
    }

    let res = res.unwrap();
    let status = res.status();

    if status.is_success() {
        let text = res.text().await.context("Could not parse response")?;
        let response = GetRequestApiResponse::Ok(GetApiResponseOk { status, text });

        Ok(response)
    } else {
        let error_response: ApiErrorResponse = res
            .json()
            .await
            .context("Failed to deserialize API error response")?;

        // Convert the API error into your custom error type
        let custom_error: CustomError = error_response.error.into();
        Ok(GetRequestApiResponse::Err(custom_error))
    }
}

pub async fn delete_request(args: RequestArgs) -> Result<DeleteRequestApiResponse> {
    let base_path = env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:5000"));

    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", base_path, args.path);

    let res = client
        .request(reqwest::Method::DELETE, full_path)
        .query(&args.query)
        .send()
        .await;

    if let Err(_) = &res {
        let err = CustomError::cannot_connect();
        bail!(err)
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
        let error_response: ApiErrorResponse = res
            .json()
            .await
            .context("Failed to deserialize API error response")?;

        // Convert the API error into your custom error type
        let custom_error: CustomError = error_response.error.into();
        Ok(DeleteRequestApiResponse::Err(custom_error))
    }
}

pub async fn post_request<T>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<RequestApiOptionResponse>
where
    T: serde::Serialize,
{
    post_patch_put(args, Some(data), reqwest::Method::POST).await
}

pub async fn patch_request<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<RequestApiOptionResponse> {
    post_patch_put(args, data, reqwest::Method::PATCH).await
}

pub async fn put_request<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<RequestApiOptionResponse> {
    post_patch_put(args, data, reqwest::Method::PUT).await
}

async fn post_patch_put<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<T>,
    method: Method,
) -> Result<RequestApiOptionResponse> {
    let base_path = env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:5000"));

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
        let err = CustomError::cannot_connect();
        bail!(err)
    }

    let res = res.unwrap();
    let status = res.status();

    if status.is_success() {
        if let Some(content_length) = res.content_length() {
            if (content_length as usize) == 0 {
                let response =
                    RequestApiOptionResponse::Ok(OptionResponseOk { status, text: None });

                Ok(response)
            } else {
                let text = res.text().await.context("Could not parse response")?;
                let response = RequestApiOptionResponse::Ok(OptionResponseOk {
                    status,
                    text: Some(text),
                });

                Ok(response)
            }
        } else {
            let response = RequestApiOptionResponse::Ok(OptionResponseOk { status, text: None });

            Ok(response)
        }
    } else {
        let error_response: ApiErrorResponse = res
            .json()
            .await
            .context("Failed to deserialize API error response")?;

        // Convert the API error into your custom error type
        let custom_error: CustomError = error_response.error.into();
        Ok(RequestApiOptionResponse::Err(custom_error))
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
