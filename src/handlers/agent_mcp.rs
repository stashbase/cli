//! Read-only inspection of configured HTTP MCP servers.

use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::Path,
    sync::{Arc, RwLock},
};

use anyhow::{bail, Context, Result};
use dialoguer::{
    theme::{ColorfulTheme, Theme},
    Confirm, MultiSelect,
};
use serde_json::{json, Value};
use spinoff::Spinner;

use crate::{
    api::secrets,
    cmd::{
        agent::{
            AgentMcpCheckCommand, AgentMcpConfigureCommand, AgentMcpToolsCommand,
            AgentMcpVerifyCommand, AgentProfileSource,
        },
        secrets::SecretsFileFormat,
    },
    config::config,
    handlers::{
        agent_policy::SecretHttpPolicy,
        agent_profiles::{
            profile_not_found_error, profile_not_found_error_with_output, source_label,
        },
        entry::root::{provision_remote_session_ca, remote_bindings, remote_session_state},
        run::proxy::{Proxy, ProxyPolicy, RemoteProxyConfig, RemoteProxyProtocol, SecretInjection},
    },
    models::{
        agent::{AgentHttpRuleEffect, AgentMcpRule, AgentMcpServer, AgentProfile},
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
    api_key: Option<&str>,
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
            AgentProfileSource::Global => global.ok_or_else(|| {
                profile_not_found_error_with_output(
                    &command.profile,
                    source_label(command.profile_source),
                    json_format,
                    silent,
                )
            })?,
            AgentProfileSource::Directory => {
                config::get_directory_agent_profile(&command.profile)?
                    .ok_or_else(|| {
                        profile_not_found_error_with_output(
                            &command.profile,
                            source_label(command.profile_source),
                            json_format,
                            silent,
                        )
                    })?
                    .profile
            }
            AgentProfileSource::Auto => config::get_directory_agent_profile(&command.profile)?
                .map(|p| p.profile)
                .or(global)
                .ok_or_else(|| {
                    profile_not_found_error_with_output(
                        &command.profile,
                        source_label(command.profile_source),
                        json_format,
                        silent,
                    )
                })?,
        }
    };
    let server = profile
        .mcp_servers
        .get(&command.server)
        .context("MCP server was not found in the profile")?;
    let url = mcp_url(server)?;
    let inspection = if command.remote {
        remote_proxied_client(&profile, server, api_key, json_format).await
    } else {
        proxied_client(&profile, server, api_key, json_format).await
    };
    let (_proxy, client, auth) = match inspection {
        Ok(value) => value,
        Err(error) => {
            if !silent {
                eprintln!();
            }
            return Err(error);
        }
    };
    let mut spinner = SpinnerGuard::new(!silent);

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
    let tools = list_mcp_tools(&client, &url, &auth, session.as_deref()).await?;
    spinner.finish();
    let rows = tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?.to_owned();
            let (allowed, reason) = tool_decision(server, &name);
            Some(json!({"name": name, "description": tool.get("description"), "allowed": allowed, "reason": reason}))
        })
        .collect::<Vec<_>>();
    let mut rows = rows;
    rows.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let mut policy_hidden_tools = server.deny_tools.clone();
    policy_hidden_tools.sort();
    if json_format {
        println!(
            "{}",
            get_formatted_json_string(
                &json!({
                    "schema_version": 1,
                    "server": command.server,
                    "url": url,
                    "binding": server.binding,
                    "tools": rows,
                    "policy_hidden_tools": policy_hidden_tools,
                }),
                true,
            )?
        );
    } else {
        println!("MCP server: {}", command.server);
        println!("Endpoint: {url}");
        println!("Binding: {}", server.binding.as_deref().unwrap_or("none"));
        let allowed = rows
            .iter()
            .filter(|row| row["allowed"].as_bool() == Some(true))
            .collect::<Vec<_>>();
        let denied = rows
            .iter()
            .filter(|row| row["allowed"].as_bool() != Some(true))
            .collect::<Vec<_>>();
        if !allowed.is_empty() {
            println!();
            println!("Allowed tools:");
            for row in allowed {
                println!("- {}", row["name"]);
            }
        }
        if !denied.is_empty() {
            println!();
            println!("Denied tools:");
            for row in denied {
                println!("- {}", row["name"]);
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct McpMultiSelectTheme {
    inner: ColorfulTheme,
}

impl Theme for McpMultiSelectTheme {
    fn format_multi_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        checked: bool,
        active: bool,
    ) -> fmt::Result {
        let cursor = if active { ">" } else { " " };
        let checkbox = if checked { "[x]" } else { "[ ]" };
        let styled_text = if active {
            self.inner.active_item_style.apply_to(text)
        } else {
            self.inner.inactive_item_style.apply_to(text)
        };
        write!(f, "{cursor} {checkbox} {styled_text}")
    }
}

pub async fn handle_agent_mcp_configure_command(
    command: AgentMcpConfigureCommand,
    global_config: &Config,
    api_key: Option<&str>,
    silent: bool,
) -> Result<()> {
    let (profile, profile_path) = load_mcp_profile_for_edit(&command, global_config)?;
    let server = profile
        .mcp_servers
        .get(&command.server)
        .cloned()
        .context("MCP server was not found in the profile")?;
    let url = mcp_url(&server)?;
    let inspection = if command.remote {
        remote_proxied_client(&profile, &server, api_key, false).await
    } else {
        proxied_client(&profile, &server, api_key, false).await
    };
    let (_proxy, client, auth) = match inspection {
        Ok(value) => value,
        Err(error) => {
            if !silent {
                eprintln!();
            }
            return Err(error);
        }
    };
    let mut spinner = SpinnerGuard::new(!silent);
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
    let mut tools = list_mcp_tools(&client, &url, &auth, session.as_deref()).await?;
    spinner.finish();
    tools.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    let names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if names.is_empty() {
        bail!("The MCP server did not expose any tools.");
    }

    println!();
    println!("MCP server: {}", command.server);
    println!("Endpoint: {url}");
    println!("Profile: {}", profile_path.display());
    println!();
    let current_allow_all = server.allow_tools.iter().any(|tool| tool == "*");
    let current_tools = server
        .allow_tools
        .iter()
        .filter(|tool| tool.as_str() != "*")
        .collect::<HashSet<_>>();
    let missing_current_tools = current_tools
        .iter()
        .filter(|tool| !names.iter().any(|name| name == **tool))
        .map(|tool| (*tool).clone())
        .collect::<Vec<_>>();
    if !missing_current_tools.is_empty() {
        eprintln!(
            "Warning: configured tools not found on the server: {}",
            missing_current_tools.join(", ")
        );
    }
    let allow_all = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Allow all tools?")
        .default(current_allow_all)
        .interact()?;
    let selected = if allow_all {
        vec!["*".to_owned()]
    } else {
        let defaults = names
            .iter()
            .map(|name| current_tools.contains(name))
            .collect::<Vec<_>>();
        let selected = MultiSelect::with_theme(&McpMultiSelectTheme::default())
            .with_prompt("Select allowed tools")
            .items(&names)
            .defaults(&defaults)
            .interact()?;
        selected
            .into_iter()
            .map(|index| names[index].clone())
            .collect::<Vec<_>>()
    };
    let summary = if selected == ["*"] {
        "all tools".to_owned()
    } else if selected.is_empty() {
        "no tools".to_owned()
    } else {
        format!(
            "{} tool{}",
            selected.len(),
            if selected.len() == 1 { "" } else { "s" }
        )
    };
    if !Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Save {summary} to the profile?"))
        .default(true)
        .interact()?
    {
        bail!("MCP profile was not changed.");
    }

    let content = fs::read_to_string(&profile_path)
        .with_context(|| format!("Could not read agent profile '{}'.", profile_path.display()))?;
    let updated = replace_mcp_allow_tools(&content, &command.server, &selected)?;
    fs::write(&profile_path, updated).with_context(|| {
        format!(
            "Could not write agent profile '{}'.",
            profile_path.display()
        )
    })?;
    println!("Updated MCP tools in {}.", profile_path.display());
    Ok(())
}

fn load_mcp_profile_for_edit(
    command: &AgentMcpConfigureCommand,
    global_config: &Config,
) -> Result<(AgentProfile, std::path::PathBuf)> {
    if let Some(path) = command.policy_file.as_deref() {
        let loaded = config::get_explicit_agent_profile(path)?;
        return Ok((loaded.profile, loaded.path));
    }
    if matches!(command.profile_source, AgentProfileSource::Global) {
        bail!("MCP configure requires a writable repository profile or --policy-file.");
    }
    if let Some(loaded) = config::get_directory_agent_profile(&command.profile)? {
        return Ok((loaded.profile, loaded.path));
    }
    if matches!(command.profile_source, AgentProfileSource::Auto)
        && global_config
            .agent_profiles
            .as_ref()
            .is_some_and(|profiles| profiles.contains_key(&command.profile))
    {
        bail!(
            "MCP configure cannot modify the global agent profile '{}'. Use --policy-file or create a repository-local profile.",
            command.profile
        );
    }
    bail!(
        "Agent profile '{}' was not found in the repository profile directory.",
        command.profile
    )
}

fn replace_mcp_allow_tools(content: &str, server: &str, tools: &[String]) -> Result<String> {
    let section = format!("[mcp_servers.{server}]");
    let lines = content.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == section)
        .context(format!(
            "MCP server '{server}' section was not found in the profile"
        ))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim_start().starts_with('['))
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    let existing = (start + 1..end).find(|index| {
        lines[*index].trim_start().starts_with("allow_tools") && lines[*index].contains('=')
    });
    let mut output = lines
        .iter()
        .map(|line| (*line).to_owned())
        .collect::<Vec<_>>();
    let indent = existing
        .map(|index| lines[index].len() - lines[index].trim_start().len())
        .unwrap_or(0);
    let replacement = format_allow_tools(indent, tools)?;
    if let Some(index) = existing {
        let existing_end = if lines[index].contains(']') {
            index
        } else {
            (index + 1..end)
                .find(|line| lines[*line].trim() == "]")
                .context("MCP allow_tools array is missing its closing bracket")?
        };
        output.splice(index..=existing_end, replacement);
    } else {
        let replacement_length = replacement.len();
        output.splice(end..end, replacement);
        if end < lines.len() {
            output.insert(end + replacement_length, String::new());
        }
    }
    let mut result = output.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn format_allow_tools(indent: usize, tools: &[String]) -> Result<Vec<String>> {
    let prefix = " ".repeat(indent);
    if tools.len() <= 3 {
        let values = tools
            .iter()
            .map(|tool| serde_json::to_string(tool))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        return Ok(vec![format!("{prefix}allow_tools = [{values}]")]);
    }

    let mut lines = vec![format!("{prefix}allow_tools = [")];
    for (index, tool) in tools.iter().enumerate() {
        let comma = if index + 1 == tools.len() { "" } else { "," };
        lines.push(format!(
            "{prefix}    {}{comma}",
            serde_json::to_string(tool)?
        ));
    }
    lines.push(format!("{prefix}]"));
    Ok(lines)
}

pub async fn handle_agent_mcp_verify_command(
    command: AgentMcpVerifyCommand,
    global_config: &Config,
    json_format: bool,
    api_key: Option<&str>,
    silent: bool,
) -> Result<bool> {
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
            AgentProfileSource::Global => global.ok_or_else(|| {
                profile_not_found_error_with_output(
                    &command.profile,
                    source_label(command.profile_source),
                    json_format,
                    silent,
                )
            })?,
            AgentProfileSource::Directory => {
                config::get_directory_agent_profile(&command.profile)?
                    .ok_or_else(|| {
                        profile_not_found_error_with_output(
                            &command.profile,
                            source_label(command.profile_source),
                            json_format,
                            silent,
                        )
                    })?
                    .profile
            }
            AgentProfileSource::Auto => config::get_directory_agent_profile(&command.profile)?
                .map(|p| p.profile)
                .or(global)
                .ok_or_else(|| {
                    profile_not_found_error_with_output(
                        &command.profile,
                        source_label(command.profile_source),
                        json_format,
                        silent,
                    )
                })?,
        }
    };
    let server = profile
        .mcp_servers
        .get(&command.server)
        .context("MCP server was not found in the profile")?;
    let url = mcp_url(server)?;
    let inspection = if command.remote {
        remote_proxied_client(&profile, server, api_key, json_format).await
    } else {
        proxied_client(&profile, server, api_key, json_format).await
    };
    let (_proxy, client, auth) = match inspection {
        Ok(value) => value,
        Err(error) => {
            if !silent {
                eprintln!();
            }
            return Err(error);
        }
    };
    let mut spinner = SpinnerGuard::new(!silent);
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
    let tools = list_mcp_tools(&client, &url, &auth, session.as_deref()).await?;
    spinner.finish();
    let available = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let configured = server
        .allow_tools
        .iter()
        .filter(|tool| tool.as_str() != "*")
        .collect::<HashSet<_>>();
    let mut missing = configured
        .iter()
        .filter(|tool| !available.contains(tool.as_str()))
        .map(|tool| (*tool).clone())
        .collect::<Vec<_>>();
    let mut available_tools = available.iter().cloned().collect::<Vec<_>>();
    available_tools.sort();
    let mut configured_tools = configured
        .iter()
        .map(|tool| (*tool).clone())
        .collect::<Vec<_>>();
    configured_tools.sort();
    missing.sort();
    let report = json!({
        "schema_version": 1,
        "profile": command.profile,
        "server": command.server,
        "url": url,
        "available_tools": available_tools,
        "configured_tools": configured_tools,
        "missing_tools": missing,
        "valid": missing.is_empty(),
    });
    if json_format {
        println!("{}", get_formatted_json_string(&report, true)?);
    } else {
        println!("MCP server: {}", command.server);
        if missing.is_empty() {
            println!("Tool verification: PASSED");
            println!("All configured tool names are available.");
        } else {
            println!("Tool verification: FAILED");
            println!("Missing configured tools ({}):", missing.len());
            for tool in missing {
                println!("  - \"{tool}\"");
            }
        }
        if !server.deny_tools.is_empty() {
            println!(
                "Denied tools were not existence-checked because the proxy hides them from tools/list."
            );
        }
    }
    Ok(!report["valid"].as_bool().unwrap_or(false))
}

fn mcp_url(server: &AgentMcpServer) -> Result<String> {
    let url = reqwest::Url::parse(&server.url).context("MCP server URL is invalid")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!(
            "MCP server URL must be an absolute http:// or https:// URL with a host and no userinfo or fragment"
        );
    }
    Ok(url.to_string())
}

struct SpinnerGuard(Option<Spinner>);

impl SpinnerGuard {
    fn new(enabled: bool) -> Self {
        Self(enabled.then(request_spinner))
    }

    fn finish(&mut self) {
        if let Some(spinner) = self.0.as_mut() {
            spinner.stop_and_persist("", "");
        }
        self.0 = None;
    }
}

impl Drop for SpinnerGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

fn mcp_endpoint_parts(server: &AgentMcpServer) -> Result<(String, String)> {
    let url = reqwest::Url::parse(&mcp_url(server)?)?;
    Ok((
        url.host_str()
            .context("MCP server URL has no host")?
            .to_owned(),
        url.path().to_owned(),
    ))
}

async fn proxied_client(
    profile: &AgentProfile,
    server: &AgentMcpServer,
    api_key: Option<&str>,
    json_format: bool,
) -> Result<(Proxy, reqwest::Client, Option<(String, String)>)> {
    let (host, path) = mcp_endpoint_parts(server)?;
    let mut mcp_rules = vec![AgentMcpRule {
        effect: AgentHttpRuleEffect::Allow,
        hosts: vec![host.clone()],
        paths: vec![path.clone()],
        tools: server.allow_tools.clone(),
    }];
    if !server.deny_tools.is_empty() {
        mcp_rules.push(AgentMcpRule {
            effect: AgentHttpRuleEffect::Deny,
            hosts: vec![host.clone()],
            paths: vec![path],
            tools: server.deny_tools.clone(),
        });
    }
    let (secrets, secret_policies, secret_injections, auth) =
        inspection_binding_policy(profile, server, &host, api_key, json_format).await?;
    let policy = ProxyPolicy {
        secret_policies,
        secret_injections,
        allowed_egress_hosts: profile
            .egress_hosts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect(),
        denied_hosts: profile
            .deny_hosts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect(),
        denied_read_paths: Vec::new(),
        denied_write_paths: Vec::new(),
        egress_hosts_configured: profile.egress_hosts.is_some(),
        strict_deny: true,
        mcp_rules: mcp_rules.clone(),
    };
    let proxy = Proxy::start_with_port(secrets, policy, None, None).await?;
    let proxy_url = proxy.child_env()["HTTPS_PROXY"].clone();
    let ca_path = proxy.child_env()["SSL_CERT_FILE"].clone();
    let ca = reqwest::Certificate::from_pem(&fs::read(ca_path)?)?;
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(proxy_url)?)
        .add_root_certificate(ca)
        .default_headers(mcp_inspection_headers(&proxy)?)
        .build()?;
    Ok((proxy, client, auth))
}

async fn remote_proxied_client(
    profile: &AgentProfile,
    server: &AgentMcpServer,
    api_key: Option<&str>,
    json_format: bool,
) -> Result<(Proxy, reqwest::Client, Option<(String, String)>)> {
    if profile.file.is_some() {
        bail!(
            "Remote MCP inspection cannot use a local profile secret file.\n\n\
             Hint: remove 'file' from the profile, use a project/environment binding, \
             or run the command without '--remote'."
        );
    }
    let api_key = api_key.context(
        "Remote MCP inspection requires a Stashbase API key.\n\n\
         Hint: run 'stashbase config api-key set' or run the command without '--remote'.",
    )?;
    let (host, _path) = mcp_endpoint_parts(server)?;
    let all_bindings = remote_bindings(profile);
    let bindings = server
        .binding
        .as_ref()
        .map(|name| {
            all_bindings
                .iter()
                .filter(|binding| &binding.name == name)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut mcp_rules = vec![crate::api::remote_proxy::RemoteMcpRule {
        effect: AgentHttpRuleEffect::Allow,
        hosts: vec![host.clone()],
        paths: vec![_path.clone()],
        tools: server.allow_tools.clone(),
    }];
    if !server.deny_tools.is_empty() {
        mcp_rules.push(crate::api::remote_proxy::RemoteMcpRule {
            effect: AgentHttpRuleEffect::Deny,
            hosts: vec![host.clone()],
            paths: vec![_path],
            tools: server.deny_tools.clone(),
        });
    }
    let request = crate::api::remote_proxy::RemoteProxySessionRequest {
        api_key: api_key.to_owned(),
        project_identifier: profile.secrets.project.clone(),
        environment_identifier: profile.secrets.environment.clone(),
        egress_hosts: profile.egress_hosts.clone().unwrap_or_default(),
        deny_hosts: profile.deny_hosts.clone().unwrap_or_default(),
        bindings: bindings.clone(),
        mcp_rules: mcp_rules.clone(),
        agent_type: None,
        session_purpose: Some("mcp_inspection".to_owned()),
        previous_session_token: None,
    };
    let session = crate::api::remote_proxy::create_session(&request, json_format).await?;
    let protocol = match session.protocol.as_str() {
        "http/1.1-custom" => RemoteProxyProtocol::Custom,
        "http/1.1-forward-proxy-tls-intercept" => RemoteProxyProtocol::ForwardProxyTlsIntercept,
        value => bail!("Agent Proxy returned an unsupported protocol: {value}"),
    };
    let ca_file = provision_remote_session_ca(&session)?;
    let state = remote_session_state(&session)?;
    let placeholders = bindings
        .iter()
        .map(|binding| (binding.name.clone(), binding.placeholder.clone()))
        .collect();
    let child_env = bindings
        .iter()
        .map(|binding| (binding.name.clone(), binding.name.clone()))
        .collect();
    let secret_policies = server
        .binding
        .as_ref()
        .map(|binding_name| {
            HashMap::from([(
                binding_name.clone(),
                SecretHttpPolicy::LegacyHosts(HashSet::from([host.clone()])),
            )])
        })
        .unwrap_or_default();
    let secret_injections = server
        .binding
        .as_ref()
        .and_then(|binding_name| {
            bindings
                .iter()
                .find(|binding| &binding.name == binding_name)
        })
        .map(|binding| {
            HashMap::from([(
                binding.name.clone(),
                SecretInjection {
                    header: server
                        .header
                        .clone()
                        .unwrap_or_else(|| binding.header.clone()),
                    value_template: server
                        .value_template
                        .clone()
                        .unwrap_or_else(|| binding.value_template.clone()),
                },
            )])
        })
        .unwrap_or_default();
    let policy = ProxyPolicy {
        secret_policies,
        secret_injections,
        allowed_egress_hosts: profile
            .egress_hosts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect(),
        denied_hosts: profile
            .deny_hosts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect(),
        denied_read_paths: Vec::new(),
        denied_write_paths: Vec::new(),
        egress_hosts_configured: profile.egress_hosts.is_some(),
        strict_deny: true,
        mcp_rules: mcp_rules
            .iter()
            .map(|rule| AgentMcpRule {
                effect: rule.effect.clone(),
                hosts: rule.hosts.clone(),
                paths: rule.paths.clone(),
                tools: rule.tools.clone(),
            })
            .collect(),
    };
    let proxy = Proxy::start_remote_with_port(
        RemoteProxyConfig {
            proxy_url: session.proxy_url,
            session: Arc::new(RwLock::new(state)),
            placeholders,
            child_env,
            protocol,
            ca_file,
        },
        policy,
        None,
        None,
    )
    .await?;
    let ca =
        reqwest::Certificate::from_pem(&fs::read(proxy.child_env()["SSL_CERT_FILE"].clone())?)?;
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(
            proxy.child_env()["HTTPS_PROXY"].clone(),
        )?)
        .add_root_certificate(ca)
        .default_headers(mcp_inspection_headers(&proxy)?)
        .build()?;
    let auth = server.binding.as_ref().and_then(|binding_name| {
        bindings
            .iter()
            .find(|binding| &binding.name == binding_name)
            .map(|binding| {
                let header = server
                    .header
                    .clone()
                    .unwrap_or_else(|| binding.header.clone());
                let value = server
                    .value_template
                    .clone()
                    .unwrap_or_else(|| binding.value_template.clone())
                    .replace("{value}", &binding.placeholder);
                (header, value)
            })
    });
    Ok((proxy, client, auth))
}

async fn inspection_binding_policy(
    profile: &AgentProfile,
    server: &AgentMcpServer,
    endpoint_host: &str,
    api_key: Option<&str>,
    json_format: bool,
) -> Result<(
    HashMap<String, String>,
    HashMap<String, SecretHttpPolicy>,
    HashMap<String, SecretInjection>,
    Option<(String, String)>,
)> {
    let Some(name) = server.binding.as_deref() else {
        return Ok((HashMap::new(), HashMap::new(), HashMap::new(), None));
    };
    let binding = profile
        .secrets
        .bindings
        .get(name)
        .or_else(|| profile.personal_credentials.get(name))
        .context(format!("MCP binding '{name}' was not found"))?;
    if profile.personal_credentials.contains_key(name) {
        bail!("MCP binding '{name}' is a personal credential and requires --remote");
    }
    let env_name = binding.env.as_deref().unwrap_or(name);
    let local_value = std::env::var(env_name).ok().or_else(|| {
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
    });
    let value = if let Some(value) = local_value {
        value
    } else if let (Some(api_key), Some(project), Some(environment)) = (
        api_key,
        profile.secrets.project.clone(),
        profile.secrets.environment.clone(),
    ) {
        let response = secrets::pull(
            api_key.to_owned(),
            Some(project),
            Some(environment),
            vec![binding.from.clone().unwrap_or_else(|| name.to_owned())],
            Vec::new(),
            false,
            false,
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let values = match response {
            crate::models::api_client::GetRequestApiResponse::Ok(data) => serde_json::from_str::<
                Vec<crate::models::secrets::SecretWithoutComment>,
            >(&data.text)?,
            crate::models::api_client::GetRequestApiResponse::Err(error) => {
                bail!(error.format_error_output(json_format)?)
            }
        };
        values
            .into_iter()
            .find(|secret| secret.name == binding.from.as_deref().unwrap_or(name))
            .map(|secret| secret.value)
            .context(format!(
                "MCP binding '{name}' was not found in the configured project/environment"
            ))?
    } else {
        return Err(anyhow::anyhow!(format!(
            "MCP binding '{name}' is not available in ${env_name} or the profile file"
        )));
    };
    let header = server
        .header
        .clone()
        .or_else(|| binding.header.clone())
        .unwrap_or_else(|| "Authorization".to_owned());
    let template = server
        .value_template
        .clone()
        .or_else(|| binding.value_template.clone())
        .unwrap_or_else(|| {
            if header.eq_ignore_ascii_case("authorization") {
                "Bearer {value}".to_owned()
            } else {
                "{value}".to_owned()
            }
        });
    let secret_policy = if binding.rules.is_empty() {
        // A named MCP binding is implicitly authorized for its configured MCP
        // endpoint. This keeps MCP authentication self-contained instead of
        // requiring a duplicate secret HTTP rule for the same host.
        SecretHttpPolicy::LegacyHosts(HashSet::from([endpoint_host.to_owned()]))
    } else {
        SecretHttpPolicy::Rules(binding.rules.clone())
    };
    let secret_injection = SecretInjection {
        header: header.clone(),
        value_template: template.clone(),
    };
    let placeholder = format!("**STASHBASE_{name}**");
    let placeholder_value = template.replace("{value}", &placeholder);
    Ok((
        HashMap::from([(name.to_owned(), value)]),
        HashMap::from([(name.to_owned(), secret_policy)]),
        HashMap::from([(name.to_owned(), secret_injection)]),
        Some((header, placeholder_value)),
    ))
}

async fn list_mcp_tools(
    client: &reqwest::Client,
    url: &str,
    auth: &Option<(String, String)>,
    session: Option<&str>,
) -> Result<Vec<Value>> {
    let mut tools = Vec::new();
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();
    let mut request_id = 2_u64;

    loop {
        let params = cursor
            .as_ref()
            .map_or_else(|| json!({}), |cursor| json!({"cursor": cursor}));
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/list",
            "params": params
        });
        let (response, _) = send_mcp(client, url, auth, request, session).await?;
        let (mut page_tools, next_cursor) = mcp_tools_page(&response)?;
        tools.append(&mut page_tools);

        let Some(next_cursor) = next_cursor else {
            return Ok(tools);
        };
        if !seen_cursors.insert(next_cursor.clone()) {
            bail!("MCP tools/list returned a repeated nextCursor");
        }
        cursor = Some(next_cursor);
        request_id = request_id.saturating_add(1);
    }
}

fn mcp_tools_page(response: &Value) -> Result<(Vec<Value>, Option<String>)> {
    if let Some(error) = response.get("error") {
        bail!("MCP tools/list failed: {error}");
    }
    let result = response
        .get("result")
        .context("MCP tools/list returned no result")?;
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .context("MCP tools/list returned no tools array")?
        .clone();
    let next_cursor = match result.get("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) => Some(cursor.clone()),
        Some(_) => bail!("MCP tools/list returned an invalid nextCursor"),
    };
    Ok((tools, next_cursor))
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

fn mcp_inspection_headers(proxy: &Proxy) -> Result<reqwest::header::HeaderMap> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-stashbase-mcp-inspection",
        reqwest::header::HeaderValue::from_str(proxy.mcp_inspection_token())?,
    );
    Ok(headers)
}

pub fn handle_agent_mcp_check_command(
    command: AgentMcpCheckCommand,
    global_config: &Config,
    json_format: bool,
) -> Result<bool> {
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
            AgentProfileSource::Global => global.ok_or_else(|| {
                profile_not_found_error(
                    &command.profile,
                    source_label(command.profile_source),
                    json_format,
                )
            })?,
            AgentProfileSource::Directory => {
                config::get_directory_agent_profile(&command.profile)?
                    .ok_or_else(|| {
                        profile_not_found_error(
                            &command.profile,
                            source_label(command.profile_source),
                            json_format,
                        )
                    })?
                    .profile
            }
            AgentProfileSource::Auto => config::get_directory_agent_profile(&command.profile)?
                .map(|p| p.profile)
                .or(global)
                .ok_or_else(|| {
                    profile_not_found_error(
                        &command.profile,
                        source_label(command.profile_source),
                        json_format,
                    )
                })?,
        }
    };
    let server = profile
        .mcp_servers
        .get(&command.server)
        .context("MCP server was not found in the profile")?;
    let (allowed, reason) = tool_decision(server, &command.tool);
    let report = json!({
        "schema_version": 1,
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
    Ok(!allowed)
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
        return (false, "allow_tools is empty; no tools are allowed");
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

#[cfg(test)]
mod tests {
    use super::{mcp_tools_page, replace_mcp_allow_tools, tool_decision};
    use crate::models::agent::AgentMcpServer;

    fn server(allow_tools: &[&str], deny_tools: &[&str]) -> AgentMcpServer {
        AgentMcpServer {
            url: "https://mcp.example.com/mcp".to_owned(),
            binding: None,
            header: None,
            value_template: None,
            allow_tools: allow_tools.iter().map(|tool| (*tool).to_owned()).collect(),
            deny_tools: deny_tools.iter().map(|tool| (*tool).to_owned()).collect(),
        }
    }

    #[test]
    fn tool_decision_defaults_to_deny_all() {
        assert_eq!(
            tool_decision(&server(&[], &[]), "read_file"),
            (false, "allow_tools is empty; no tools are allowed")
        );
    }

    #[test]
    fn tool_decision_supports_wildcards_and_deny_precedence() {
        assert_eq!(
            tool_decision(&server(&["*"], &["delete_file"]), "read_file"),
            (true, "matched allow_tools")
        );
        assert_eq!(
            tool_decision(&server(&["*"], &["delete_file"]), "delete_file"),
            (false, "matched deny_tools")
        );
    }

    #[test]
    fn tool_decision_rejects_tools_outside_allow_list() {
        assert_eq!(
            tool_decision(&server(&["read_file"], &[]), "write_file"),
            (false, "not present in allow_tools")
        );
    }

    #[test]
    fn mcp_tools_page_returns_tools_and_next_cursor() {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [{"name": "first"}],
                "nextCursor": "next-page"
            }
        });

        let (tools, cursor) = mcp_tools_page(&response).unwrap();
        assert_eq!(tools, vec![serde_json::json!({"name": "first"})]);
        assert_eq!(cursor.as_deref(), Some("next-page"));
    }

    #[test]
    fn mcp_tools_page_stops_without_a_next_cursor() {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {"tools": [{"name": "last"}]}
        });

        let (_, cursor) = mcp_tools_page(&response).unwrap();
        assert_eq!(cursor, None);
    }

    #[test]
    fn configure_replaces_only_the_selected_mcp_allowlist() {
        let profile = "# profile\n\n[mcp_servers.linear]\nurl = \"https://mcp.example.com/mcp\"\n# keep me\nallow_tools = [\"old_tool\"]\n\n[secrets.TOKEN]\nenv = \"TOKEN\"\n";
        let updated = replace_mcp_allow_tools(
            profile,
            "linear",
            &["list_issues".to_owned(), "get_issue".to_owned()],
        )
        .unwrap();

        assert!(updated.contains("# profile"));
        assert!(updated.contains("# keep me"));
        assert!(updated.contains("allow_tools = [\"list_issues\", \"get_issue\"]"));
        assert!(!updated.contains("allow_tools = [\"old_tool\"]"));
        assert!(updated.contains("[secrets.TOKEN]"));
    }

    #[test]
    fn configure_inserts_allowlist_when_it_is_missing() {
        let profile = "[mcp_servers.linear]\nurl = \"https://mcp.example.com/mcp\"\n\n[secrets.TOKEN]\nenv = \"TOKEN\"\n";
        let updated = replace_mcp_allow_tools(profile, "linear", &["*".to_owned()]).unwrap();

        assert!(updated.contains("allow_tools = [\"*\"]\n\n[secrets.TOKEN]"));
    }

    #[test]
    fn configure_formats_large_allowlists_as_multiline_toml() {
        let profile = "[mcp_servers.linear]\nurl = \"https://mcp.example.com/mcp\"\n";
        let tools = ["one", "two", "three", "four"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let updated = replace_mcp_allow_tools(profile, "linear", &tools).unwrap();

        assert!(updated.contains(
            "allow_tools = [\n    \"one\",\n    \"two\",\n    \"three\",\n    \"four\"\n]"
        ));
    }

    #[test]
    fn configure_replaces_the_entire_existing_multiline_allowlist() {
        let profile = "[mcp_servers.linear]\nurl = \"https://mcp.example.com/mcp\"\nallow_tools = [\n    \"old_one\",\n    \"old_two\"\n]\n\n[secrets.TOKEN]\nenv = \"TOKEN\"\n";
        let updated = replace_mcp_allow_tools(
            profile,
            "linear",
            &["new_one".to_owned(), "new_two".to_owned()],
        )
        .unwrap();

        assert!(updated.contains("allow_tools = [\"new_one\", \"new_two\"]"));
        assert!(!updated.contains("old_one"));
        assert!(!updated.contains("old_two"));
        assert_eq!(updated.matches("allow_tools").count(), 1);
        assert!(updated.contains("[secrets.TOKEN]"));
        assert!(toml::from_str::<crate::models::agent::AgentProfile>(&updated).is_ok());
    }
}
