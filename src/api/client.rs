use std::env;

use reqwest::{header::HeaderMap, Client, ClientBuilder, RequestBuilder};

use crate::models::api_client::GetRequestArgs;

pub fn build_client() -> Client {
    let builder = ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("env-ease-cli/0.0.1");

    let client = builder.build().unwrap();
    client
}

pub fn get_request(args: GetRequestArgs) -> RequestBuilder {
    let base_path =
        env::var("HERO_API_URL").unwrap_or_else(|_| format!("http://localhost:8080/api/v1/cli"));

    let client = build_client();
    let full_path = format!("{}/{}", base_path, args.path);

    let mut headers = HeaderMap::new();
    headers.insert("token", args.token.parse().unwrap());

    client
        .request(reqwest::Method::GET, full_path)
        .headers(headers)
}
