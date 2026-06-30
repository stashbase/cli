use anyhow::{bail, Result};
use tabled::Tabled;

use crate::{
    api::auth::get_current_auth_details,
    cmd::config::OutputFormat,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        auth::CurrentAuthResponse,
    },
    utils::{output::get_formatted_json_string, spinner::request_spinner, tables},
};

// for whoami command
pub struct GetCurrentAuthDetailsRequestArgs {
    pub api_key: String,
    pub format: OutputFormat,
    pub silent: bool,
}

pub async fn handle_whoami_command(args: GetCurrentAuthDetailsRequestArgs) -> Result<()> {
    let GetCurrentAuthDetailsRequestArgs {
        api_key,
        format,
        silent,
    } = args;

    let json_format = format == OutputFormat::Json;

    let spinner = if !silent {
        Some(request_spinner())
    } else {
        None
    };

    let response = get_current_auth_details(api_key).await;

    if let Err(err) = response {
        if let Some(mut spinner) = spinner {
            spinner.stop_and_persist("", "");
        }

        let formatted_err = err.format_error_output(json_format)?;
        bail!(formatted_err);
    }

    let response = response.unwrap();

    match response {
        GetRequestApiResponse::Ok(data) => {
            let auth_details = serde_json::from_str::<CurrentAuthResponse>(&data.text);
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            match auth_details {
                Ok(auth_details) => match format {
                    OutputFormat::Json => {
                        let pretty = get_formatted_json_string(&auth_details, true).unwrap();
                        println!("{}", pretty);
                    }
                    OutputFormat::Plain => {
                        print!("{}", auth_details);
                    }
                    OutputFormat::Table => {
                        let rows = whoami_table_rows(&auth_details);
                        let table = tables::build::build_table(&rows);
                        println!("{}", table.to_string());
                    }
                },
                Err(_) => {
                    let error = OutputError::failed_to_deserialize_response_body();
                    let formatted_err = error.format_error_output(json_format)?;

                    bail!(formatted_err);
                }
            }
        }
        GetRequestApiResponse::Err(err) => {
            if let Some(mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }

            let formatted_err = err.format_error_output(json_format)?;
            bail!(formatted_err);
        }
    }

    Ok(())
}

#[derive(Tabled)]
struct WhoamiTableRow {
    #[tabled(rename = "Field", order = 0)]
    field: String,
    #[tabled(rename = "Value", order = 1)]
    value: String,
}

fn whoami_table_rows(auth: &CurrentAuthResponse) -> Vec<WhoamiTableRow> {
    let mut rows = Vec::new();

    match auth {
        CurrentAuthResponse::User { data } => {
            rows.push(row("Type", "user"));
            rows.push(row("ID", &data.id));
            rows.push(row("Email", &data.email));
            rows.push(row("Full Name", &data.full_name));
            if let Some(display_name) = &data.display_name {
                rows.push(row("Display Name", display_name));
            }
            rows.push(row("Workspace ID", &data.workspace.id));
            rows.push(row("Workspace Name", &data.workspace.name));
            rows.push(row("Workspace Slug", &data.workspace.slug));
            if let Some(role) = &data.workspace.user_role {
                rows.push(row("Workspace Role", &role.to_string()));
            }
        }
        CurrentAuthResponse::EnvironmentAccount { data } => {
            rows.push(row("Type", "environment_account"));
            rows.push(row("ID", &data.id));
            rows.push(row("Name", &data.name));
            rows.push(row("Workspace ID", &data.workspace.id));
            rows.push(row("Workspace Name", &data.workspace.name));
            rows.push(row("Workspace Slug", &data.workspace.slug));
            rows.push(row("Project ID", &data.project.id));
            rows.push(row("Project Name", &data.project.name));
            rows.push(row("Environment ID", &data.project.environment.id));
            rows.push(row("Environment Name", &data.project.environment.name));

            let mut permission_entries: Vec<_> = data.permissions.iter().collect();
            permission_entries.sort_by(|a, b| a.0.cmp(b.0));
            for (resource, actions) in permission_entries {
                rows.push(row(
                    &format!("Permission ({})", resource),
                    &actions.join(", "),
                ));
            }
        }
        CurrentAuthResponse::ServiceAccount { data } => {
            rows.push(row("Type", "service_account"));
            rows.push(row("ID", &data.id));
            rows.push(row("Name", &data.name));
            rows.push(row("Workspace ID", &data.workspace.id));
            rows.push(row("Workspace Name", &data.workspace.name));
            rows.push(row("Workspace Slug", &data.workspace.slug));

            if let Some(access) = &data.access {
                rows.push(row("Project Count", &access.project_count.to_string()));

                if let Some(workspace_access) = &access.workspace {
                    push_permissions(
                        &mut rows,
                        "Workspace Permissions",
                        workspace_access.permissions.as_ref(),
                    );
                    push_permissions(
                        &mut rows,
                        "Created Project Permissions",
                        workspace_access.created_project_permissions.as_ref(),
                    );
                    push_permissions(
                        &mut rows,
                        "Created Environment Permissions",
                        workspace_access.created_environment_permissions.as_ref(),
                    );
                }
            }
        }
    }

    rows
}

fn push_permissions(
    rows: &mut Vec<WhoamiTableRow>,
    label_prefix: &str,
    permissions: Option<&std::collections::HashMap<String, Vec<String>>>,
) {
    if let Some(permissions) = permissions {
        let mut permission_entries: Vec<_> = permissions.iter().collect();
        permission_entries.sort_by(|a, b| a.0.cmp(b.0));
        for (resource, actions) in permission_entries {
            rows.push(row(
                &format!("{} ({})", label_prefix, resource),
                &actions.join(", "),
            ));
        }
    }
}

fn row(field: &str, value: &str) -> WhoamiTableRow {
    WhoamiTableRow {
        field: field.to_string(),
        value: value.to_string(),
    }
}
