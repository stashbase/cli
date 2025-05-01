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

impl std::fmt::Display for WorkspaceUserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceUserRole::MEMBER => write!(f, "Member"),
            WorkspaceUserRole::ADMIN => write!(f, "Admin"),
            WorkspaceUserRole::OWNER => write!(f, "Owner"),
        }
    }
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

impl std::fmt::Display for CurrentAuthResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let CurrentAuthResponse::User { data } = self {
            writeln!(f, "Authenticated as User:")?;
            writeln!(f, "  ID: {}", data.id)?;
            writeln!(f, "  Email: {}", data.email)?;
            writeln!(f, "  Name: {}", data.name)?;
            writeln!(f, "  Workspace:")?;
            writeln!(f, "    ID: {}", data.workspace.id)?;
            writeln!(f, "    Name: {}", data.workspace.name)?;
            writeln!(f, "    Slug: {}", data.workspace.slug)?;
            writeln!(f, "  Workspace User Role: {}", data.workspace_user_role)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for AuthedEnvironmentAccountData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Authenticated as Environment Account:")?;
        writeln!(f, "  ID: {}", self.id)?;
        writeln!(f, "  Name: {}", self.name)?;
        writeln!(f, "  Environment:")?;
        writeln!(f, "    ID: {}", self.environment.id)?;
        writeln!(f, "    Name: {}", self.environment.name)?;
        writeln!(f, "  Project:")?;
        writeln!(f, "    ID: {}", self.project.id)?;
        writeln!(f, "    Name: {}", self.project.name)?;
        writeln!(f, "  Workspace:")?;
        writeln!(f, "    ID: {}", self.workspace.id)?;
        writeln!(f, "    Name: {}", self.workspace.name)?;
        writeln!(f, "    Slug: {}", self.workspace.slug)?;
        writeln!(f, "  Permissions:")?;

        for (key, value) in &self.permissions {
            writeln!(f, "    {}: {}", key, value.join(", "))?;
        }
        Ok(())
    }
}

impl std::fmt::Display for AuthedServiceAccountData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Authenticated as Service Account:")?;
        writeln!(f, "  ID: {}", self.id)?;
        writeln!(f, "  Name: {}", self.name)?;
        writeln!(f, "  Workspace:")?;
        writeln!(f, "    ID: {}", self.workspace.id)?;
        writeln!(f, "    Name: {}", self.workspace.name)?;
        writeln!(f, "    Slug: {}", self.workspace.slug)?;

        if let Some(access) = &self.access {
            writeln!(f, "  Access:")?;
            writeln!(f, "    Project Count: {}", access.project_count)?;

            if let Some(workspace) = &access.workspace {
                writeln!(f, "    Workspace Permissions:")?;

                if let Some(permissions) = &workspace.permissions {
                    for (key, value) in permissions {
                        writeln!(f, "      {}: {}", key, value.join(", "))?;
                    }
                } else {
                    writeln!(f, "      None")?;
                }

                if let Some(created_project_permissions) = &workspace.created_project_permissions {
                    writeln!(f, "    Created Project Permissions:")?;
                    for (key, value) in created_project_permissions {
                        writeln!(f, "      {}: {}", key, value.join(", "))?;
                    }
                }

                if let Some(created_environment_permissions) =
                    &workspace.created_environment_permissions
                {
                    writeln!(f, "    Created Environment Permissions:")?;
                    for (key, value) in created_environment_permissions {
                        writeln!(f, "      {}: {}", key, value.join(", "))?;
                    }
                }
            }
        }

        Ok(())
    }
}
