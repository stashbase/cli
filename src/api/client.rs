use std::env;
use std::sync::atomic::Ordering;
use std::time::Duration;

use reqwest::{header::HeaderMap, Method};
use reqwest_middleware::RequestBuilder;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{
    default_on_request_failure, policies::ExponentialBackoff, RetryTransientMiddleware, Retryable,
    RetryableStrategy,
};
use tokio::time::sleep;

use crate::models::api_client::{
    ApiErrorResponse, DeleteApiResponseOk, DeleteRequestApiResponse, GenericOutputError,
    GetApiResponseOk, GetRequestApiResponse, OptionResponseOk, OutputError,
    RequestApiOptionResponse, RequestArgs,
};
use crate::{REQUEST_ABORTED, REQUEST_TIMEOUT_SECS};

const DEFAULT_API_URL: &str = "https://api.stashbase.com";
const API_URL_ENV_VAR: &str = "STASHBASE_API_URL";
const BUILD_TIME_API_URL: Option<&str> = option_env!("STASHBASE_API_URL");

fn get_api_url() -> String {
    env::var(API_URL_ENV_VAR)
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            BUILD_TIME_API_URL
                .map(|v| v.trim().trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
}

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

    let authorization_header_value = format!("Bearer {}", api_key);

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", authorization_header_value.parse().unwrap());

    let builder = ClientBuilder::new(
        reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(get_request_timeout_secs()))
            .default_headers(headers)
            .user_agent("stashbase/cli/0.1.0")
            .build()
            .unwrap(),
    )
    .with(ret_s);

    let client = builder.build();
    client
}

pub fn build_client_no_retry(api_key: String) -> ClientWithMiddleware {
    let authorization_header_value = format!("Bearer {}", api_key);

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", authorization_header_value.parse().unwrap());

    ClientBuilder::new(
        reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(get_request_timeout_secs()))
            .default_headers(headers)
            .user_agent("stashbase/cli/0.1.0")
            .build()
            .unwrap(),
    )
    .build()
}

fn get_request_timeout_secs() -> u64 {
    REQUEST_TIMEOUT_SECS.get().copied().unwrap_or(30)
}

fn map_send_error(error: &reqwest_middleware::Error) -> OutputError {
    if REQUEST_ABORTED.load(Ordering::SeqCst) {
        return OutputError::request_aborted();
    }

    let error_message = error.to_string().to_ascii_lowercase();
    if error_message.contains("timed out") || error_message.contains("timeout") {
        return OutputError::request_timed_out();
    }

    OutputError::cannot_connect()
}

async fn wait_for_abort() {
    loop {
        if REQUEST_ABORTED.load(Ordering::SeqCst) {
            return;
        }

        sleep(Duration::from_millis(50)).await;
    }
}

async fn send_with_abort(builder: RequestBuilder) -> Result<reqwest::Response, OutputError> {
    if REQUEST_ABORTED.load(Ordering::SeqCst) {
        return Err(OutputError::request_aborted());
    }

    let send_future = builder.send();

    tokio::select! {
        res = send_future => {
            res.map_err(|err| map_send_error(&err))
        }
        _ = wait_for_abort() => {
            Err(OutputError::request_aborted())
        }
    }
}

pub async fn get_request(args: RequestArgs) -> Result<GetRequestApiResponse, OutputError> {
    let client = build_client(args.api_key.clone());
    get_request_with_client(client, args).await
}

pub async fn get_request_no_retry(args: RequestArgs) -> Result<GetRequestApiResponse, OutputError> {
    let client = build_client_no_retry(args.api_key.clone());
    get_request_with_client(client, args).await
}

async fn get_request_with_client(
    client: ClientWithMiddleware,
    args: RequestArgs,
) -> Result<GetRequestApiResponse, OutputError> {
    let full_path = format!("{}/{}", get_api_url(), args.path);

    let res = send_with_abort(
        client
            .request(reqwest::Method::GET, full_path)
            .query(&args.query),
    )
    .await?;
    let status = res.status();

    if status.is_success() {
        let text = res
            .text()
            .await
            .map_err(|_| OutputError::failed_to_read_response_body())?;
        let response = GetRequestApiResponse::Ok(GetApiResponseOk { status, text });

        Ok(response)
    } else {
        if status == 503 {
            let err = OutputError::Generic(GenericOutputError {
                code: Some("server.temporary_unavailable".to_string()),
                message: format!("API service is temporarily unavailable. Please try again later."),
                hint: None,
                details: None,
            });
            return Err(err);
        }

        let error_response: ApiErrorResponse = res
            .json()
            .await
            .map_err(|_| OutputError::failed_to_deserialize_response_body())?;

        // Convert the API error into your custom error type
        let custom_error: OutputError = error_response.error.into();
        Ok(GetRequestApiResponse::Err(custom_error))
    }
}

pub async fn delete_request(args: RequestArgs) -> Result<DeleteRequestApiResponse, OutputError> {
    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", get_api_url(), args.path);

    let res = send_with_abort(
        client
            .request(reqwest::Method::DELETE, full_path)
            .query(&args.query),
    )
    .await?;
    let status = res.status();

    if status.is_success() {
        if let Some(content_length) = res.content_length() {
            if (content_length as usize) == 0 {
                let response =
                    DeleteRequestApiResponse::Ok(DeleteApiResponseOk { status, text: None });

                Ok(response)
            } else {
                let text = res
                    .text()
                    .await
                    .map_err(|_| OutputError::failed_to_read_response_body())?;
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
        if status == 503 {
            let err = OutputError::Generic(GenericOutputError {
                code: Some("server.temporary_unavailable".to_string()),
                message: format!("API service is temporarily unavailable. Please try again later."),
                hint: None,
                details: None,
            });
            return Err(err);
        }

        let error_response: ApiErrorResponse = res
            .json()
            .await
            .map_err(|_| OutputError::failed_to_deserialize_response_body())?;

        // Convert the API error into your custom error type
        let custom_error: OutputError = error_response.error.into();
        Ok(DeleteRequestApiResponse::Err(custom_error))
    }
}

pub async fn post_request<T>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<RequestApiOptionResponse, OutputError>
where
    T: serde::Serialize,
{
    post_patch_put(args, Some(data), reqwest::Method::POST).await
}

pub async fn patch_request<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<RequestApiOptionResponse, OutputError> {
    post_patch_put(args, data, reqwest::Method::PATCH).await
}

pub async fn put_request<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<&T>,
) -> Result<RequestApiOptionResponse, OutputError> {
    post_patch_put(args, data, reqwest::Method::PUT).await
}

async fn post_patch_put<T: serde::Serialize>(
    args: RequestArgs,
    data: Option<T>,
    method: Method,
) -> Result<RequestApiOptionResponse, OutputError> {
    let client = build_client(args.api_key);
    let full_path = format!("{}/{}", get_api_url(), args.path);

    let mut headers = HeaderMap::new();

    let request_builder = match data {
        Some(data) => {
            headers.insert("Content-Type", "application/json".parse().unwrap());

            client
                .request(method, full_path)
                .headers(headers)
                .query(&args.query)
                .body(serde_json::to_string(&data).unwrap())
        }
        None => client
            .request(method, full_path)
            .headers(headers)
            .query(&args.query),
    };
    let res = send_with_abort(request_builder).await?;
    let status = res.status();

    if status.is_success() {
        if let Some(content_length) = res.content_length() {
            if (content_length as usize) == 0 {
                let response =
                    RequestApiOptionResponse::Ok(OptionResponseOk { status, text: None });

                Ok(response)
            } else {
                let text = res
                    .text()
                    .await
                    .map_err(|_| OutputError::failed_to_read_response_body())?;

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
        if status == 503 {
            let err = OutputError::Generic(GenericOutputError {
                code: Some("server.temporary_unavailable".to_string()),
                message: format!("API service is temporarily unavailable. Please try again later."),
                hint: None,
                details: None,
            });
            return Err(err);
        }

        let error_response: ApiErrorResponse = res
            .json()
            .await
            .map_err(|_| OutputError::failed_to_deserialize_response_body())?;

        // Convert the API error into your custom error type
        let custom_error: OutputError = error_response.error.into();
        Ok(RequestApiOptionResponse::Err(custom_error))
    }
}
