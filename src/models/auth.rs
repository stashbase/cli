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
        match self {
            CurrentAuthResponse::User { data } => write!(f, "{}", data),
            CurrentAuthResponse::ServiceAccount { data } => write!(f, "{}", data),
            CurrentAuthResponse::EnvironmentAccount { data } => write!(f, "{}", data),
        }
    }
}

impl std::fmt::Display for AuthedUserData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Authenticated as User:")?;
        write_indented(f, 2, &format!("ID: {}", self.id))?;
        write_indented(f, 2, &format!("Email: {}", self.email))?;
        write_indented(f, 2, &format!("Name: {}", self.name))?;
        write_indented(f, 2, "Workspace:")?;
        write_indented(f, 4, &format!("ID: {}", self.workspace.id))?;
        write_indented(f, 4, &format!("Name: {}", self.workspace.name))?;
        write_indented(f, 4, &format!("Slug: {}", self.workspace.slug))?;
        write_indented(f, 4, &format!("User Role: {}", self.workspace_user_role))?;

        Ok(())
    }
}

impl std::fmt::Display for AuthedEnvironmentAccountData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::utils::output::write_indented;
        writeln!(f, "Authenticated as Environment Account:")?;
        write_indented(f, 2, &format!("ID: {}", self.id))?;
        write_indented(f, 2, &format!("Name: {}", self.name))?;
        write_indented(f, 2, "Environment:")?;
        write_indented(f, 4, &format!("ID: {}", self.environment.id))?;
        write_indented(f, 4, &format!("Name: {}", self.environment.name))?;
        write_indented(f, 2, "Project:")?;
        write_indented(f, 4, &format!("ID: {}", self.project.id))?;
        write_indented(f, 4, &format!("Name: {}", self.project.name))?;
        write_indented(f, 2, "Workspace:")?;
        write_indented(f, 4, &format!("ID: {}", self.workspace.id))?;
        write_indented(f, 4, &format!("Name: {}", self.workspace.name))?;
        write_indented(f, 4, &format!("Slug: {}", self.workspace.slug))?;
        write_indented(f, 2, "Permissions:")?;
        for (key, value) in &self.permissions {
            write_indented(f, 4, &format!("{}: {}", key, value.join(", ")))?;
        }
        Ok(())
    }
}

use crate::utils::output::write_indented;

impl std::fmt::Display for AuthedServiceAccountData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Authenticated as Service Account:")?;
        write_indented(f, 2, &format!("ID: {}", self.id))?;
        write_indented(f, 2, &format!("Name: {}", self.name))?;
        write_indented(f, 2, "Workspace:")?;
        write_indented(f, 4, &format!("ID: {}", self.workspace.id))?;
        write_indented(f, 4, &format!("Name: {}", self.workspace.name))?;
        write_indented(f, 4, &format!("Slug: {}", self.workspace.slug))?;

        if let Some(access) = &self.access {
            write_indented(f, 2, "Access:")?;
            write_indented(f, 4, &format!("Project Count: {}", access.project_count))?;

            if let Some(workspace) = &access.workspace {
                write_indented(f, 4, "Workspace:")?;
                write_indented(f, 6, "Permissions:")?;

                if let Some(permissions) = &workspace.permissions {
                    for (key, value) in permissions {
                        write_indented(f, 8, &format!("{}: {}", key, value.join(", ")))?;
                    }
                } else {
                    write_indented(f, 8, "None")?;
                }

                if let Some(created_project_permissions) = &workspace.created_project_permissions {
                    write_indented(f, 6, "Created Project Permissions:")?;
                    for (key, value) in created_project_permissions {
                        write_indented(f, 8, &format!("{}: {}", key, value.join(", ")))?;
                    }
                }

                if let Some(created_environment_permissions) =
                    &workspace.created_environment_permissions
                {
                    write_indented(f, 6, "Created Environment Permissions:")?;
                    for (key, value) in created_environment_permissions {
                        write_indented(f, 8, &format!("{}: {}", key, value.join(", ")))?;
                    }
                }
            }
        }

        Ok(())
    }
}
