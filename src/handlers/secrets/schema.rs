use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::{
    api::{environments, projects, secrets},
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        environments::Environment,
        projects::SingleProject,
        secrets::SecretMetadataListResponse,
    },
    utils::interaction,
};

pub struct HandlePullSecretSchemaArgs {
    pub api_key: String,
    pub project: String,
    pub environment: String,
    pub output: PathBuf,
    pub force: bool,
    pub silent: bool,
}

#[derive(Serialize)]
struct SecretSchemaExport {
    project: SchemaProject,
    environment: SchemaEnvironment,
    secrets: Vec<SchemaSecret>,
}

#[derive(Serialize)]
struct SchemaProject {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Serialize)]
struct SchemaEnvironment {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_production: Option<bool>,
}

#[derive(Serialize)]
struct SchemaSecret {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}

pub async fn handle_pull_secret_schema(args: HandlePullSecretSchemaArgs) -> Result<()> {
    if args.output.exists() && !args.force {
        if args.silent {
            bail!(
                "refusing to overwrite existing file {}; pass --force to overwrite",
                args.output.display()
            );
        }

        eprintln!("Warning: file {} already exists.", args.output.display());
        if interaction::confirm_opt("Do you want to overwrite it?") != Some(true) {
            return Ok(());
        }
    }

    let project: SingleProject = parse_response(api_response(
        projects::get_project(args.api_key.clone(), args.project.clone()).await,
    )?)?;
    let environment: Environment = parse_response(api_response(
        environments::get(
            args.api_key.clone(),
            Some(args.project.clone()),
            Some(args.environment.clone()),
        )
        .await,
    )?)?;
    let secret_metadata: SecretMetadataListResponse = parse_response(api_response(
        secrets::list_secret_metadata(args.api_key, Some(args.project), Some(args.environment))
            .await,
    )?)?;

    let export = SecretSchemaExport {
        project: SchemaProject {
            id: project.id,
            name: project.name,
            description: non_empty(project.description),
        },
        environment: SchemaEnvironment {
            id: environment.id,
            name: environment.name,
            description: non_empty(environment.description),
            is_production: environment.is_production,
        },
        secrets: secret_metadata
            .secrets
            .into_iter()
            .map(|secret| SchemaSecret {
                name: secret.name,
                comment: non_empty(secret.comment),
            })
            .collect(),
    };

    let yaml = serialize_schema_yaml(&export)?;
    if let Some(parent) = args
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    fs::write(&args.output, yaml)
        .with_context(|| format!("failed to write {}", args.output.display()))?;

    if !args.silent {
        println!(
            "Exported {} secret{} to {}",
            export.secrets.len(),
            if export.secrets.len() == 1 { "" } else { "s" },
            args.output.display()
        );
    }

    Ok(())
}

fn parse_response<T: serde::de::DeserializeOwned>(response: GetRequestApiResponse) -> Result<T> {
    match response {
        GetRequestApiResponse::Ok(data) => serde_json::from_str(&data.text).map_err(|_| {
            anyhow::anyhow!(OutputError::failed_to_deserialize_response_body().to_string())
        }),
        GetRequestApiResponse::Err(error) => bail!(error.format_error_output(false)?),
    }
}

fn api_response(
    response: Result<GetRequestApiResponse, OutputError>,
) -> Result<GetRequestApiResponse> {
    response.map_err(|error| match error.format_error_output(false) {
        Ok(message) => anyhow::anyhow!(message),
        Err(error) => anyhow::anyhow!(error),
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn serialize_schema_yaml(export: &SecretSchemaExport) -> Result<String> {
    let project = indent_yaml(&serde_yaml::to_string(&export.project)?);
    let environment = indent_yaml(&serde_yaml::to_string(&export.environment)?);
    let secrets = if export.secrets.is_empty() {
        "secrets: []\n".to_owned()
    } else {
        format!("secrets:\n{}", serialize_secrets_yaml(&export.secrets)?)
    };

    Ok(format!(
        "project:\n{project}\nenvironment:\n{environment}\n{secrets}"
    ))
}

fn indent_yaml(yaml: &str) -> String {
    yaml.lines().map(|line| format!("  {line}\n")).collect()
}

fn serialize_secrets_yaml(secrets: &[SchemaSecret]) -> Result<String> {
    let mut yaml = String::new();

    for (index, secret) in secrets.iter().enumerate() {
        if index > 0 && (secret.comment.is_some() || secrets[index - 1].comment.is_some()) {
            yaml.push('\n');
        }

        let serialized = serde_yaml::to_string(secret)?;
        for (line_index, line) in serialized.lines().enumerate() {
            if line_index == 0 {
                yaml.push_str("- ");
            } else {
                yaml.push_str("  ");
            }
            yaml.push_str(line);
            yaml.push('\n');
        }
    }

    Ok(yaml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::secrets::SecretMetadata;

    #[test]
    fn yaml_export_contains_only_safe_metadata() {
        let secret_metadata: SecretMetadataListResponse = serde_json::from_str(
            r#"{
                "secrets": [{
                    "name": "STRIPE_SECRET_KEY",
                    "comment": "Used to create payments and refunds.",
                    "value": "sk_live_secret_value",
                    "version": 1,
                    "has_value": true,
                    "created_at": "2026-01-01",
                    "updated_at": "2026-01-01",
                    "last_accessed_at": null
                }, {
                    "name": "SENTRY_DSN",
                    "comment": null,
                    "version": 1,
                    "has_value": true,
                    "created_at": "2026-01-01",
                    "updated_at": "2026-01-01",
                    "last_accessed_at": null
                }, {
                    "name": "ANOTHER_SECRET",
                    "comment": "Documented for agents.",
                    "version": 1,
                    "has_value": true,
                    "created_at": "2026-01-01",
                    "updated_at": "2026-01-01",
                    "last_accessed_at": null
                }]
            }"#,
        )
        .unwrap();
        let export = SecretSchemaExport {
            project: SchemaProject {
                id: "project_123".to_owned(),
                name: "api".to_owned(),
                description: Some("Main customer API.".to_owned()),
            },
            environment: SchemaEnvironment {
                id: "environment_456".to_owned(),
                name: "production".to_owned(),
                description: Some("Customer-facing environment.".to_owned()),
                is_production: Some(true),
            },
            secrets: secret_metadata
                .secrets
                .into_iter()
                .map(|secret| SchemaSecret {
                    name: secret.name,
                    comment: non_empty(secret.comment),
                })
                .collect(),
        };

        let yaml = serialize_schema_yaml(&export).unwrap();
        assert!(yaml.contains("name: api"));
        assert!(yaml.contains("id: project_123"));
        assert!(yaml.contains("id: environment_456"));
        assert!(yaml.contains("is_production: true"));
        assert!(yaml.contains("name: STRIPE_SECRET_KEY"));
        assert!(!yaml.contains("sk_live_secret_value"));
        assert!(!yaml.contains("created_at"));
        assert!(!yaml.contains("version:"));
        assert!(yaml.contains("Main customer API.\n\nenvironment:"));
        assert!(yaml.contains("is_production: true\n\nsecrets:"));
        assert!(
            yaml.contains("comment: Used to create payments and refunds.\n\n- name: SENTRY_DSN")
        );
        assert!(yaml.contains("name: SENTRY_DSN\n\n- name: ANOTHER_SECRET"));
    }

    #[test]
    fn empty_optional_metadata_is_omitted() {
        let secret = SecretMetadata {
            name: "SENTRY_DSN".to_owned(),
            comment: Some("   ".to_owned()),
            version: 1,
            has_value: true,
            created_at: "2026-01-01".to_owned(),
            updated_at: "2026-01-01".to_owned(),
            last_accessed_at: None,
        };
        let yaml = serde_yaml::to_string(&SchemaSecret {
            name: secret.name,
            comment: non_empty(secret.comment),
        })
        .unwrap();

        assert_eq!(yaml, "name: SENTRY_DSN\n");
    }

    #[test]
    fn is_production_is_omitted_when_absent() {
        let export = SecretSchemaExport {
            project: SchemaProject {
                id: "project_123".to_owned(),
                name: "api".to_owned(),
                description: None,
            },
            environment: SchemaEnvironment {
                id: "environment_456".to_owned(),
                name: "preview".to_owned(),
                description: None,
                is_production: None,
            },
            secrets: Vec::new(),
        };

        let yaml = serialize_schema_yaml(&export).unwrap();
        assert!(!yaml.contains("is_production"));
    }
}
