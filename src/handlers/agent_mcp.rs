//! Read-only inspection of configured HTTP MCP servers.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{Arc, RwLock},
};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use spinoff::Spinner;

use crate::{
    api::secrets,
    cmd::{
        agent::{
            AgentMcpCheckCommand, AgentMcpToolsCommand, AgentMcpVerifyCommand, AgentProfileSource,
        },
        secrets::SecretsFileFormat,
    },
    config::config,
    handlers::{
        agent_policy::SecretHttpPolicy,
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
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
    let (response, _) = send_mcp(&client, &url, &auth, list, session.as_deref()).await?;
    spinner.finish();
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
                    "tools": rows,
                    "policy_hidden_tools": policy_hidden_tools,
                }),
                true,
            )?
        );
    } else {
        println!("MCP server: {}", command.server);
        println!("Endpoint: {url}");
        println!();
        println!("Allowed tools:");
        for row in rows
            .iter()
            .filter(|row| row["allowed"].as_bool() == Some(true))
        {
            println!("- {}", row["name"]);
        }
        println!();
        println!("Denied tools:");
        for row in rows
            .iter()
            .filter(|row| row["allowed"].as_bool() != Some(true))
        {
            if row["reason"] == "not present in allow_tools" {
                println!("- {}", row["name"]);
            } else {
                println!("- {}  ({})", row["name"], row["reason"]);
            }
        }
    }
    Ok(())
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
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
    let (response, _) = send_mcp(&client, &url, &auth, list, session.as_deref()).await?;
    spinner.finish();
    if let Some(error) = response.get("error") {
        bail!("MCP tools/list failed: {error}");
    }
    let available = response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .context("MCP tools/list returned no tools array")?
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
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
    let mut available_tools = available
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect::<Vec<_>>();
    available_tools.sort();
    let mut configured_tools = configured
        .iter()
        .map(|tool| (*tool).clone())
        .collect::<Vec<_>>();
    configured_tools.sort();
    missing.sort();
    let mut policy_hidden_tools = server.deny_tools.clone();
    policy_hidden_tools.sort();
    let report = json!({
        "schema_version": 1,
        "profile": command.profile,
        "server": command.server,
        "url": url,
        "available_tools": available_tools,
        "configured_tools": configured_tools,
        "missing_tools": missing,
        "policy_hidden_tools": policy_hidden_tools,
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
        tools: if server.allow_tools.is_empty() {
            vec!["*".to_owned()]
        } else {
            server.allow_tools.clone()
        },
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
        bail!("--remote MCP inspection does not support local profile secret files");
    }
    let api_key = api_key.context("--remote MCP inspection requires a Stashbase API key")?;
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
        tools: if server.allow_tools.is_empty() {
            vec!["*".to_owned()]
        } else {
            server.allow_tools.clone()
        },
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
        .header("x-stashbase-mcp-inspection", "1")
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
        .header("x-stashbase-mcp-inspection", "1")
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
