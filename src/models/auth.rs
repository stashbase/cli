use crate::utils::output::{write_indented, ColorizeIfColoredOutput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceData {
    pub id: String,
    pub slug: String,
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_role: Option<WorkspaceUserRole>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceUserRole {
    Member,
    Admin,
    Owner,
}

impl std::fmt::Display for WorkspaceUserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceUserRole::Member => write!(f, "Member"),
            WorkspaceUserRole::Admin => write!(f, "Admin"),
            WorkspaceUserRole::Owner => write!(f, "Owner"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedUserData {
    pub id: String,
    pub email: String,
    pub full_name: String,
    pub display_name: Option<String>,
    pub workspace: WorkspaceData,
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
    pub environment: EnvironmentData,
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
    pub permissions: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthedEnvironmentAccountResponse {
    #[serde(rename = "type")]
    pub type_field: String, // always "environment_account"
    pub data: AuthedEnvironmentAccountData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceAccountWorkspaceAccess {
    pub permissions: Option<HashMap<String, Vec<String>>>,
    pub created_project_permissions: Option<HashMap<String, Vec<String>>>,
    pub created_environment_permissions: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Serialize, Deserialize)]
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
            CurrentAuthResponse::User { data } => {
                let message = "Authenticated as User:".blue_bold_if_tty();
                writeln!(f, "{}", message)?;
                write!(f, "{}", data)?;
                Ok(())
            }
            CurrentAuthResponse::ServiceAccount { data } => {
                let message = "Authenticated as Service Account:".blue_bold_if_tty();
                writeln!(f, "{}", message)?;
                write!(f, "{}", data)?;
                Ok(())
            }
            CurrentAuthResponse::EnvironmentAccount { data } => {
                let message = "Authenticated as Environment Account:".blue_bold_if_tty();
                writeln!(f, "{}", message)?;
                write!(f, "{}", data)?;
                Ok(())
            }
        }
    }
}

impl std::fmt::Display for AuthedUserData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indented(f, 2, &format!("{} {}", "ID:".blue_bold_if_tty(), self.id))?;
        write_indented(
            f,
            2,
            &format!("{} {}", "Email:".blue_bold_if_tty(), self.email),
        )?;
        write_indented(
            f,
            2,
            &format!("{} {}", "Full Name:".blue_bold_if_tty(), self.full_name),
        )?;
        if let Some(display_name) = &self.display_name {
            write_indented(
                f,
                2,
                &format!("{} {}", "Display Name:".blue_bold_if_tty(), display_name),
            )?;
        }
        write_indented(f, 2, &"Workspace:".blue_bold_if_tty())?;
        write_indented(f, 4, &format!("{} {}", "ID:".blue_bold_if_tty(), self.workspace.id))?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Name:".blue_bold_if_tty(), self.workspace.name),
        )?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Slug:".blue_bold_if_tty(), self.workspace.slug),
        )?;
        if let Some(user_role) = &self.workspace.user_role {
            write_indented(
                f,
                4,
                &format!("{} {}", "User Role:".blue_bold_if_tty(), user_role),
            )?;
        }

        Ok(())
    }
}

impl std::fmt::Display for AuthedEnvironmentAccountData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indented(f, 2, &format!("{} {}", "ID:".blue_bold_if_tty(), self.id))?;
        write_indented(f, 2, &format!("{} {}", "Name:".blue_bold_if_tty(), self.name))?;
        write_indented(f, 2, &"Project:".blue_bold_if_tty())?;
        write_indented(f, 4, &format!("{} {}", "ID:".blue_bold_if_tty(), self.project.id))?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Name:".blue_bold_if_tty(), self.project.name),
        )?;
        write_indented(f, 4, &"Environment:".blue_bold_if_tty())?;
        write_indented(
            f,
            6,
            &format!(
                "{} {}",
                "ID:".blue_bold_if_tty(),
                self.project.environment.id
            ),
        )?;
        write_indented(
            f,
            6,
            &format!(
                "{} {}",
                "Name:".blue_bold_if_tty(),
                self.project.environment.name
            ),
        )?;
        write_indented(f, 2, &"Workspace:".blue_bold_if_tty())?;
        write_indented(f, 4, &format!("{} {}", "ID:".blue_bold_if_tty(), self.workspace.id))?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Name:".blue_bold_if_tty(), self.workspace.name),
        )?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Slug:".blue_bold_if_tty(), self.workspace.slug),
        )?;
        write_indented(f, 2, &"Permissions:".blue_bold_if_tty())?;
        for (key, value) in &self.permissions {
            write_indented(
                f,
                4,
                &format!("{} {}", format!("{}:", key).blue_bold_if_tty(), value.join(", ")),
            )?;
        }
        Ok(())
    }
}

impl std::fmt::Display for AuthedServiceAccountData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_indented(f, 2, &format!("{} {}", "ID:".blue_bold_if_tty(), self.id))?;
        write_indented(f, 2, &format!("{} {}", "Name:".blue_bold_if_tty(), self.name))?;
        write_indented(f, 2, &"Workspace:".blue_bold_if_tty())?;
        write_indented(f, 4, &format!("{} {}", "ID:".blue_bold_if_tty(), self.workspace.id))?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Name:".blue_bold_if_tty(), self.workspace.name),
        )?;
        write_indented(
            f,
            4,
            &format!("{} {}", "Slug:".blue_bold_if_tty(), self.workspace.slug),
        )?;

        if let Some(access) = &self.access {
            write_indented(f, 2, &"Access:".blue_bold_if_tty())?;
            write_indented(
                f,
                4,
                &format!(
                    "{} {}",
                    "Project Count:".blue_bold_if_tty(),
                    access.project_count
                ),
            )?;

            if let Some(workspace) = &access.workspace {
                write_indented(f, 4, &"Workspace:".blue_bold_if_tty())?;
                write_indented(f, 6, &"Permissions:".blue_bold_if_tty())?;

                if let Some(permissions) = &workspace.permissions {
                    for (key, value) in permissions {
                        write_indented(
                            f,
                            8,
                            &format!("{} {}", format!("{}:", key).blue_bold_if_tty(), value.join(", ")),
                        )?;
                    }
                } else {
                    write_indented(f, 8, "None")?;
                }

                if let Some(created_project_permissions) = &workspace.created_project_permissions {
                    write_indented(f, 6, &"Created Project Permissions:".blue_bold_if_tty())?;
                    for (key, value) in created_project_permissions {
                        write_indented(
                            f,
                            8,
                            &format!("{} {}", format!("{}:", key).blue_bold_if_tty(), value.join(", ")),
                        )?;
                    }
                }

                if let Some(created_environment_permissions) =
                    &workspace.created_environment_permissions
                {
                    write_indented(
                        f,
                        6,
                        &"Created Environment Permissions:".blue_bold_if_tty(),
                    )?;
                    for (key, value) in created_environment_permissions {
                        write_indented(
                            f,
                            8,
                            &format!("{} {}", format!("{}:", key).blue_bold_if_tty(), value.join(", ")),
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}
