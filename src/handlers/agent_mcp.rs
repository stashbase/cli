//! Read-only inspection of configured HTTP MCP servers.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::{
    cmd::{
        agent::{AgentMcpCheckCommand, AgentMcpToolsCommand, AgentProfileSource},
        secrets::SecretsFileFormat,
    },
    config::config,
    models::{
        agent::{AgentMcpServer, AgentProfile},
        config::Config,
    },
    utils::{
        output::get_formatted_json_string, secrets::read_secrets_from_file,
        spinner::request_spinner,
    },
};

pub async fn handle_agent_mcp_tools_command(
    command: AgentMcpToolsCommand,
    global_config: &Config,
    json_format: bool,
    silent: bool,
) -> Result<()> {
    let explicit = command
        .policy_file
        .as_deref()
        .map(config::get_explicit_agent_profile)
        .transpose()?;
    let global = global_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let profile = if let Some(profile) = explicit {
        profile.profile
    } else {
        match command.profile_source {
            AgentProfileSource::Global => global.context("agent profile was not found")?,
            AgentProfileSource::Directory => {
                config::get_directory_agent_profile(&command.profile)?
                    .context("agent profile was not found")?
                    .profile
            }
            AgentProfileSource::Auto => config::get_directory_agent_profile(&command.profile)?
                .map(|p| p.profile)
                .or(global)
                .context("agent profile was not found")?,
        }
    };
    let server = profile
        .mcp_servers
        .get(&command.server)
        .context("MCP server was not found in the profile")?;
    let url = mcp_url(server)?;
    let client = reqwest::Client::builder().build()?;
    let auth = binding_header(&profile, server)?;
    let mut spinner = (!silent).then(request_spinner);

    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "stashbase", "version": env!("CARGO_PKG_VERSION")}}
    });
    let (initialize_response, session) = send_mcp(&client, &url, &auth, initialize, None).await?;
    if let Some(error) = initialize_response.get("error") {
        bail!("MCP initialize failed: {error}");
    }
    send_mcp_notification(
        &client,
        &url,
        &auth,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        session.as_deref(),
    )
    .await?;
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
    let (response, _) = send_mcp(&client, &url, &auth, list, session.as_deref()).await?;
    if let Some(spinner) = spinner.as_mut() {
        spinner.stop_and_persist("", "");
    }
    if let Some(error) = response.get("error") {
        bail!("MCP tools/list failed: {error}");
    }
    let tools = response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .context("MCP tools/list returned no tools array")?;
    let rows = tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?.to_owned();
            let (allowed, _) = tool_decision(server, &name);
            Some(json!({"name": name, "description": tool.get("description"), "allowed": allowed}))
        })
        .collect::<Vec<_>>();
    if json_format {
        println!(
            "{}",
            get_formatted_json_string(
                &json!({"server": command.server, "url": url, "tools": rows}),
                true,
            )?
        );
    } else {
        println!("MCP server: {}", command.server);
        println!("Endpoint: {url}");
        for row in rows {
            println!(
                "{} {}",
                if row["allowed"].as_bool().unwrap_or(false) {
                    "✓"
                } else {
                    "✗"
                },
                row["name"]
            );
        }
    }
    Ok(())
}

fn mcp_url(server: &AgentMcpServer) -> Result<String> {
    let host = server.hosts.first().context("MCP server has no host")?;
    let path = server.paths.first().map(String::as_str).unwrap_or("/");
    Ok(format!(
        "https://{}{}",
        host.trim_end_matches('/'),
        if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        }
    ))
}

fn binding_header(
    profile: &AgentProfile,
    server: &AgentMcpServer,
) -> Result<Option<(String, String)>> {
    let Some(name) = server.binding.as_deref() else {
        return Ok(None);
    };
    let binding = profile
        .secrets
        .bindings
        .get(name)
        .or_else(|| profile.personal_credentials.get(name))
        .context(format!("MCP binding '{name}' was not found"))?;
    if profile.personal_credentials.contains_key(name) {
        bail!("MCP binding '{name}' is a personal credential and cannot be read by the local CLI");
    }
    let env_name = binding.env.as_deref().unwrap_or(name);
    let value = std::env::var(env_name)
        .ok()
        .or_else(|| {
            profile
                .file
                .as_deref()
                .and_then(|file| {
                    read_secrets_from_file(Path::new(file), &SecretsFileFormat::Dotenv).ok()
                })
                .and_then(|items| {
                    items
                        .into_iter()
                        .find(|item| item.name == env_name)
                        .map(|item| item.value)
                })
        })
        .context(format!(
            "MCP binding '{name}' is not available in ${env_name} or the profile file"
        ))?;
    let header = binding
        .header
        .clone()
        .unwrap_or_else(|| "Authorization".to_owned());
    let template = binding.value_template.clone().unwrap_or_else(|| {
        if header.eq_ignore_ascii_case("authorization") {
            "Bearer {value}".to_owned()
        } else {
            "{value}".to_owned()
        }
    });
    Ok(Some((header, template.replace("{value}", &value))))
}

async fn send_mcp(
    client: &reqwest::Client,
    url: &str,
    auth: &Option<(String, String)>,
    body: Value,
    session: Option<&str>,
) -> Result<(Value, Option<String>)> {
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-03-26")
        .json(&body);
    if let Some((header, value)) = auth {
        request = request.header(header, value);
    }
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request.send().await?.error_for_status()?;
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let text = response.text().await?;
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .unwrap_or(&text)
        .trim();
    Ok((
        serde_json::from_str(payload).context("MCP response was not valid JSON")?,
        session,
    ))
}

async fn send_mcp_notification(
    client: &reqwest::Client,
    url: &str,
    auth: &Option<(String, String)>,
    body: Value,
    session: Option<&str>,
) -> Result<()> {
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-03-26")
        .json(&body);
    if let Some((header, value)) = auth {
        request = request.header(header, value);
    }
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    request.send().await?.error_for_status()?;
    Ok(())
}

pub fn handle_agent_mcp_check_command(
    command: AgentMcpCheckCommand,
    global_config: &Config,
    json_format: bool,
) -> Result<()> {
    let explicit = command
        .policy_file
        .as_deref()
        .map(config::get_explicit_agent_profile)
        .transpose()?;
    let global = global_config
        .agent_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&command.profile))
        .cloned();
    let profile = if let Some(profile) = explicit {
        profile.profile
    } else {
        match command.profile_source {
            AgentProfileSource::Global => global.context("agent profile was not found")?,
            AgentProfileSource::Directory => {
                config::get_directory_agent_profile(&command.profile)?
                    .context("agent profile was not found")?
                    .profile
            }
            AgentProfileSource::Auto => config::get_directory_agent_profile(&command.profile)?
                .map(|p| p.profile)
                .or(global)
                .context("agent profile was not found")?,
        }
    };
    let server = profile
        .mcp_servers
        .get(&command.server)
        .context("MCP server was not found in the profile")?;
    let (allowed, reason) = tool_decision(server, &command.tool);
    let report = json!({
        "profile": command.profile,
        "server": command.server,
        "tool": command.tool,
        "allowed": allowed,
        "decision": if allowed { "allowed" } else { "denied" },
        "reason": reason,
    });
    if json_format {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("MCP server: {}", command.server);
        println!("Tool: {}", command.tool);
        println!("Decision: {}", if allowed { "ALLOWED" } else { "DENIED" });
        println!("Reason: {reason}");
    }
    Ok(())
}

fn tool_decision(server: &AgentMcpServer, name: &str) -> (bool, &'static str) {
    if server
        .deny_tools
        .iter()
        .any(|tool| tool == "*" || tool == name)
    {
        return (false, "matched deny_tools");
    }
    if server.allow_tools.is_empty() {
        return (true, "allow_tools is empty; all tools are allowed");
    }
    if server
        .allow_tools
        .iter()
        .any(|tool| tool == "*" || tool == name)
    {
        (true, "matched allow_tools")
    } else {
        (false, "not present in allow_tools")
    }
}
