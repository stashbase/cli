use crate::{
    api::client,
    models::{
        api_client::{ApiPath, OutputError, RequestApiOptionResponse, RequestArgs},
        dependencies::{
            DependencyCheckBatchRequest, DependencyCheckBatchResponse, DependencyCheckRequest,
        },
    },
};

pub async fn check_batch(
    api_key: String,
    dependencies: Vec<DependencyCheckRequest>,
) -> Result<DependencyCheckBatchResponse, OutputError> {
    let request = DependencyCheckBatchRequest { dependencies };
    let args = RequestArgs {
        api_key,
        path: ApiPath::DependenciesCheck,
        query: None,
    };
    match client::post_request(args, Some(&request)).await? {
        RequestApiOptionResponse::Ok(ok) => parse_response(
            ok.text
                .as_deref()
                .ok_or_else(OutputError::failed_to_read_response_body)?,
        ),
        RequestApiOptionResponse::Err(error) => Err(error),
    }
}

fn parse_response(
    text: &str,
) -> Result<crate::models::dependencies::DependencyCheckBatchResponse, OutputError> {
    serde_json::from_str(text).map_err(|_| OutputError::failed_to_deserialize_response_body())
}

#[cfg(test)]
mod tests {
    use super::parse_response;
    use crate::models::dependencies::{DependencyCheckRequest, DependencyDecision};

    #[test]
    fn batch_request_contains_only_dependency_pairs() {
        let request =
            serde_json::to_value(crate::models::dependencies::DependencyCheckBatchRequest {
                dependencies: vec![DependencyCheckRequest::new("lodash", "4.17.21").unwrap()],
            })
            .unwrap();
        assert_eq!(
            request,
            serde_json::json!({
                "dependencies": [{"name": "lodash", "version": "4.17.21"}]
            })
        );
    }

    #[test]
    fn preserves_batch_decision_and_individual_results() {
        let response = parse_response(
            r#"{"decision":"block","dependencies":[{"package":{"name":"lodash","version":"4.17.21"},"decision":"block","severity":"high","findings":[{"code":"CVE-1","severity":"high","reason":"unsafe"}],"reasons":["unsafe"],"references":{"osv":["https://example.test/CVE-1"],"npm":"https://example.test/lodash"}}]}"#,
        )
        .unwrap();
        assert_eq!(response.decision, DependencyDecision::Block);
        assert_eq!(response.dependencies[0].package.name, "lodash");
    }
}
