use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCheckRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCheckBatchRequest {
    pub dependencies: Vec<DependencyCheckRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DependencyDecision {
    Allow,
    Warn,
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DependencySeverity {
    Info,
    Low,
    Moderate,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyPackage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyFinding {
    pub code: String,
    pub severity: DependencySeverity,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyReferences {
    pub osv: Vec<String>,
    pub npm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyCheckResponse {
    pub package: DependencyPackage,
    pub decision: DependencyDecision,
    pub severity: DependencySeverity,
    pub findings: Vec<DependencyFinding>,
    pub reasons: Vec<String>,
    pub references: DependencyReferences,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyCheckBatchResponse {
    #[serde(alias = "results")]
    pub dependencies: Vec<DependencyCheckResponse>,
    pub decision: DependencyDecision,
}

impl DependencyCheckRequest {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        let version = version.into();
        if !is_npm_package_name(&name) {
            return Err("Package name must be an npm package name.".to_string());
        }
        if !is_exact_npm_version(&version) {
            return Err("Version must be an exact npm version, such as 1.2.3.".to_string());
        }
        Ok(Self {
            name,
            version: Some(version),
        })
    }

    pub fn latest(name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        if !is_npm_package_name(&name) {
            return Err("Package name must be an npm package name.".to_string());
        }
        Ok(Self {
            name,
            version: None,
        })
    }
}

fn is_npm_package_name(name: &str) -> bool {
    if let Some(scoped) = name.strip_prefix('@') {
        let parts: Vec<_> = scoped.split('/').collect();
        return parts.len() == 2 && parts.iter().all(|part| valid_package_part(part));
    }
    valid_package_part(name) && !name.contains('/')
}

fn valid_package_part(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with(['.', '_'])
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn is_exact_npm_version(version: &str) -> bool {
    let (core, build) = version
        .split_once('+')
        .map_or((version, None), |(c, b)| (c, Some(b)));
    if build.is_some_and(|value| !valid_version_identifiers(value, false)) {
        return false;
    }
    let (core, prerelease) = core
        .split_once('-')
        .map_or((core, None), |(core, pre)| (core, Some(pre)));
    if prerelease.is_some_and(|value| !valid_version_identifiers(value, true)) {
        return false;
    }
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part == &"0" || !part.starts_with('0'))
                && part.chars().all(|c| c.is_ascii_digit())
        })
}

fn valid_version_identifiers(value: &str, prerelease: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && (!prerelease
                    || !part.chars().all(|c| c.is_ascii_digit())
                    || part == "0"
                    || !part.starts_with('0'))
        })
}

#[cfg(test)]
mod tests {
    use super::DependencyCheckRequest;

    #[test]
    fn accepts_exact_npm_versions() {
        for version in ["1.2.3", "0.0.0-alpha.1", "1.2.3+build.7"] {
            assert!(DependencyCheckRequest::new("lodash", version).is_ok());
        }
    }

    #[test]
    fn rejects_non_exact_versions() {
        for version in [
            "latest", "^1.2.3", "~1.2.3", ">=1.2.3", "1.2", "1.02.3", "git:foo",
        ] {
            assert!(DependencyCheckRequest::new("lodash", version).is_err());
        }
    }

    #[test]
    fn accepts_scoped_package_names() {
        assert!(DependencyCheckRequest::new("@scope/package", "1.2.3").is_ok());
    }

    #[test]
    fn omits_version_for_latest_checks() {
        let request = DependencyCheckRequest::latest("lodash").unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({"name": "lodash"})
        );
    }
}
