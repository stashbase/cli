use std::env;

use anyhow::{bail, Context};
use reqwest::{header::HeaderMap, Client, ClientBuilder, RequestBuilder};

use crate::models::api_client::{
    ApiErrorResponse, CustomError, GetRequestApiResponse, GetRequestApiResponseOk, GetRequestArgs,
};

pub fn build_client() -> Client {
    let builder = ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("env-ease-cli/0.0.1");

    let client = builder.build().unwrap();
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
//     headers.insert("token", args.token.parse().unwrap());
//
//     client
//         .request(reqwest::Method::GET, full_path)
//         .headers(headers)
// }
//

pub async fn get_request(args: GetRequestArgs) -> anyhow::Result<GetRequestApiResponse> {
    let base_path =
        env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));

    let client = build_client();
    let full_path = format!("{}/{}", base_path, args.path);

    let mut headers = HeaderMap::new();
    headers.insert("token", args.token.parse().unwrap());

    let res = client
        .request(reqwest::Method::GET, full_path)
        .headers(headers)
        .send()
        .await;

    if let Err(_) = &res {
        bail!("Could not connect to API")
    }

    let res = res.unwrap();
    let status = res.status();

    if status.is_success() {
        let text = res.text().await.context("Could not parse response")?;
        let response = GetRequestApiResponse::Ok(GetRequestApiResponseOk { status, text });

        Ok(response)
    } else {
        if status == 401 {
            bail!("Unauthorized")
        } else {
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

pub fn post_request<T>(args: GetRequestArgs, data: Option<T>) -> RequestBuilder
where
    T: serde::Serialize,
{
    let base_path =
        env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));

    let client = build_client();
    let full_path = format!("{}/{}", base_path, args.path);

    let mut headers = HeaderMap::new();
    headers.insert("token", args.token.parse().unwrap());

    match data {
        Some(data) => {
            headers.insert("Content-Type", "application/json".parse().unwrap());

            client
                .request(reqwest::Method::POST, full_path)
                .headers(headers)
                .json(&data)
        }
        None => client
            .request(reqwest::Method::POST, full_path)
            .headers(headers),
    }
}
