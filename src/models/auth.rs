use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceData {
    pub id: String,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WorkspaceUserRole {
    MEMBER,
    ADMIN,
    OWNER,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthedUserData {
    pub id: String,
    pub email: String,
    pub name: String,
    pub workspace: WorkspaceData,
    pub workspace_user_role: WorkspaceUserRole,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedUserResponse {
    #[serde(rename = "type")]
    pub type_field: String, // always "user"
    pub data: AuthedUserData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectData {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentData {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedEnvironmentAccountData {
    pub id: String,
    pub name: String,
    pub workspace: WorkspaceData,
    pub project: ProjectData,
    pub environment: EnvironmentData,
    pub permissions: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedEnvironmentAccountResponse {
    #[serde(rename = "type")]
    pub type_field: String, // always "environment_account"
    pub data: AuthedEnvironmentAccountData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountWorkspaceAccess {
    pub permissions: Option<HashMap<String, Vec<String>>>,
    pub created_project_permissions: Option<HashMap<String, Vec<String>>>,
    pub created_environment_permissions: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountAccess {
    pub workspace: Option<ServiceAccountWorkspaceAccess>,
    pub project_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedServiceAccountData {
    pub id: String,
    pub name: String,
    pub workspace: WorkspaceData,
    pub access: Option<ServiceAccountAccess>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedServiceAccountResponse {
    #[serde(rename = "type")]
    pub type_field: String, // always "service_account"
    pub data: AuthedServiceAccountData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CurrentAuthResponse {
    User { data: AuthedUserData },
    EnvironmentAccount { data: AuthedEnvironmentAccountData },
    ServiceAccount { data: AuthedServiceAccountData },
}
