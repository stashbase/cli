//! Read-only inspection of configured HTTP MCP servers.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt, fs,
    path::Path,
    sync::{Arc, RwLock},
};

use anyhow::{bail, Context, Result};
use dialoguer::{
    theme::{ColorfulTheme, Theme},
    Confirm, MultiSelect,
};
use owo_colors::OwoColorize;
use serde_json::{json, Value};

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
        agent::policy::SecretHttpPolicy,
        agent::profiles::{
            profile_not_found_error, profile_not_found_error_with_output, source_label,
        },
        entry::root::{provision_remote_session_ca, remote_bindings, remote_session_state},
        run::proxy::{Proxy, ProxyPolicy, RemoteProxyConfig, RemoteProxyProtocol, SecretInjection},
    },
    models::{
        agent::{
            AgentBindingProfile, AgentFieldCondition, AgentHttpRuleEffect, AgentMcpRule,
            AgentMcpServer, AgentProfile, AgentScalar,
        },
        config::Config,
        validation::{AgentSessionInputValidationError, InputValidationError},
    },
    utils::{
        output::{get_formatted_json_string, is_color_enabled},
        secrets::read_secrets_from_file,
        spinner::{request_spinner, Spinner},
    },
};

/// Agent input errors print as `{"error": ...}` under --json, like a missing
/// profile does.
fn agent_input_error(error: AgentSessionInputValidationError, json: bool) -> anyhow::Error {
    let error = InputValidationError::AgentSession(error);
    let fallback = error.to_string();
    anyhow::anyhow!(error.format_error_output(json).unwrap_or(fallback))
}

fn mcp_server<'a>(
    profile: &'a AgentProfile,
    server: &str,
    json: bool,
) -> Result<&'a AgentMcpServer> {
    profile.mcp_servers.get(server).ok_or_else(|| {
        agent_input_error(
            AgentSessionInputValidationError::McpServerNotFound {
                server: server.to_owned(),
            },
            json,
        )
    })
}

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
    let server = mcp_server(&profile, &command.server, json_format)?;
    let url = mcp_url(server)?;
    let mut spinner = SpinnerGuard::new(!silent);
    let inspection = if command.remote {
        remote_proxied_client(&profile, server, api_key, json_format).await
    } else {
        proxied_client(&profile, server, api_key, json_format).await
    };
    let (_proxy, client, auth) = match inspection {
        Ok(value) => value,
        Err(error) => return Err(error),
    };

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
    let warnings = tool_rule_warnings(server, &tools);
    if let Some(name) = command.tool.as_deref() {
        let tool = tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| {
                agent_input_error(
                    AgentSessionInputValidationError::McpToolNotFound {
                        server: command.server.clone(),
                        tool: name.to_owned(),
                    },
                    json_format,
                )
            })?;
        return print_tool_details(&command.server, server, tool, &warnings, json_format);
    }
    let rows = tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?.to_owned();
            let (allowed, reason) = tool_decision(server, &name);
            Some(json!({
                "name": name,
                "description": tool.get("description"),
                "allowed": allowed,
                "reason": reason,
                "input_schema": tool.get("inputSchema"),
            }))
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
                    "warnings": warnings,
                }),
                true,
            )?
        );
    } else {
        let paint = Paint::new();
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
            println!("{}", paint.header("Allowed tools:"));
            for row in allowed {
                println!("- {}", row["name"]);
            }
        }
        if !denied.is_empty() {
            println!();
            println!("{}", paint.header("Denied tools:"));
            for row in denied {
                println!("- {}", row["name"]);
            }
        }
        print_warnings(&warnings, paint);
        println!();
        println!(
            "{}",
            paint.dim("Show a tool's arguments with --tool <NAME>.")
        );
    }
    Ok(())
}

/// One argument of an MCP tool, flattened from its JSON Schema `inputSchema`
/// into the dotted paths that tool rule `arguments` use.
#[derive(Debug, PartialEq)]
struct ToolArgument {
    path: String,
    kind: String,
    required: bool,
    values: Vec<String>,
    description: Option<String>,
}

fn tool_arguments(schema: &Value) -> Vec<ToolArgument> {
    let mut arguments = Vec::new();
    collect_tool_arguments(schema, "", &mut arguments);
    arguments
}

fn collect_tool_arguments(schema: &Value, prefix: &str, arguments: &mut Vec<ToolArgument>) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    for (name, property) in properties {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        arguments.push(ToolArgument {
            path: path.clone(),
            kind: schema_type(property),
            required: required.contains(name.as_str()),
            values: property
                .get("enum")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| match value {
                            Value::String(value) => value.clone(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            description: property
                .get("description")
                .and_then(Value::as_str)
                .and_then(|description| description.lines().next())
                .map(|line| line.trim().to_owned())
                .filter(|line| !line.is_empty()),
        });
        collect_tool_arguments(property, &path, arguments);
    }
}

/// A short type label such as `string`, `number[]` or `string|number`.
fn schema_type(schema: &Value) -> String {
    let variants = |types: Vec<String>| {
        let mut types = types
            .into_iter()
            .filter(|kind| kind != "null")
            .collect::<Vec<_>>();
        types.dedup();
        if types.is_empty() {
            "null".to_owned()
        } else {
            types.join("|")
        }
    };
    match schema.get("type") {
        Some(Value::String(kind)) if kind == "array" => format!(
            "{}[]",
            schema.get("items").map_or("any".to_owned(), schema_type)
        ),
        Some(Value::String(kind)) => kind.clone(),
        Some(Value::Array(kinds)) => variants(
            kinds
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
        _ => match schema
            .get("anyOf")
            .or_else(|| schema.get("oneOf"))
            .and_then(Value::as_array)
        {
            Some(options) => variants(options.iter().map(schema_type).collect()),
            None => "any".to_owned(),
        },
    }
}

/// Profile mistakes that only the server's tool list reveals: tools the
/// server does not offer, and argument fields a tool does not declare. A
/// misspelled field never matches, so its allow rule denies every call.
fn tool_rule_warnings(server: &AgentMcpServer, tools: &[Value]) -> Vec<String> {
    let find = |name: &str| {
        tools
            .iter()
            .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
    };
    let mut warnings = Vec::new();
    for name in server.allow_tools.iter().filter(|name| *name != "*") {
        if find(name).is_none() {
            warnings.push(format!(
                "allow_tools names '{name}', which the server does not offer."
            ));
        }
    }
    for (rule, tool) in server.shadowed_tool_conditions() {
        warnings.push(format!(
            "Tool rule {rule}: {}",
            crate::handlers::agent::validate::shadowed_condition_message(&tool)
        ));
    }
    for (index, rule) in server.tool_rules.iter().enumerate() {
        for name in rule.tools.iter().filter(|name| *name != "*") {
            let Some(tool) = find(name) else {
                warnings.push(format!(
                    "Tool rule {} names '{name}', which the server does not offer.",
                    index + 1
                ));
                continue;
            };
            let schema = tool.get("inputSchema").unwrap_or(&Value::Null);
            if schema.get("properties").is_none() {
                continue;
            }
            let declared = tool_arguments(schema)
                .into_iter()
                .map(|argument| argument.path)
                .collect::<HashSet<_>>();
            for path in rule
                .arguments
                .keys()
                .chain(rule.allow_fields.iter().flatten())
            {
                if !declared.contains(path) {
                    warnings.push(format!(
                        "Tool rule {}: arguments.{path} is not an argument of '{name}'.",
                        index + 1
                    ));
                }
            }
        }
    }
    warnings
}

/// Conditions plus, in strict mode, the other fields the rule allows.
fn format_rule_conditions(
    arguments: &BTreeMap<String, AgentFieldCondition>,
    allow_fields: Option<&[String]>,
) -> String {
    let conditions = format_conditions(arguments);
    match allow_fields {
        None => conditions,
        Some(fields) => {
            let only = if fields.is_empty() {
                "no other fields".to_owned()
            } else {
                format!("other fields only {}", fields.join(", "))
            };
            [conditions, only]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

fn format_conditions(arguments: &BTreeMap<String, AgentFieldCondition>) -> String {
    let scalar = |value: &AgentScalar| match value {
        AgentScalar::Bool(value) => value.to_string(),
        AgentScalar::Integer(value) => value.to_string(),
        AgentScalar::Float(value) => value.to_string(),
        AgentScalar::String(value) => format!("{value:?}"),
    };
    arguments
        .iter()
        .map(|(path, condition)| match condition {
            AgentFieldCondition::Equals(value) => format!("{path} = {}", scalar(value)),
            AgentFieldCondition::OneOf(values) => format!(
                "{path} = [{}]",
                values.iter().map(scalar).collect::<Vec<_>>().join(", ")
            ),
            AgentFieldCondition::Range(range) => {
                let bounds = [("min", range.min), ("max", range.max)]
                    .into_iter()
                    .filter_map(|(name, bound)| bound.map(|bound| format!("{name} = {bound}")))
                    .collect::<Vec<_>>();
                format!("{path} = {{ {} }}", bounds.join(", "))
            }
            AgentFieldCondition::Absent(_) => format!("{path} = {{ absent = true }}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One entry of the server's policy that decides calls of a tool.
#[derive(Debug, PartialEq)]
struct RuleLine {
    /// `1. ` for tool rules; empty for `allow_tools` / `deny_tools`.
    number: String,
    effect: &'static str,
    /// Argument conditions; `None` for `allow_tools` / `deny_tools`.
    conditions: Option<String>,
}

impl RuleLine {
    /// The printed line, with `effect:` padded to `effect_width` so conditions align.
    fn render(&self, effect_width: usize, paint: Paint) -> String {
        let effect = match self.effect {
            "allow" | "allow_tools" => paint.allowed_soft(self.effect),
            _ => paint.denied_soft(self.effect),
        };
        match &self.conditions {
            None => format!("{}{effect}", self.number),
            Some(conditions) => {
                let padding = " ".repeat(effect_width.saturating_sub(self.effect.len()));
                format!("{}{effect}: {padding}{conditions}", self.number)
            }
        }
    }
}

/// Which parts of the server entry decide calls of `name`, in profile order.
fn tool_rule_lines(server: &AgentMcpServer, name: &str) -> Vec<RuleLine> {
    let names_tool = |tools: &[String]| tools.iter().any(|tool| tool == "*" || tool == name);
    let mut lines = Vec::new();
    for (listed, effect) in [
        (&server.allow_tools, "allow_tools"),
        (&server.deny_tools, "deny_tools"),
    ] {
        if names_tool(listed) {
            lines.push(RuleLine {
                number: String::new(),
                effect,
                conditions: None,
            });
        }
    }
    for (index, rule) in server.tool_rules.iter().enumerate() {
        if names_tool(&rule.tools) {
            lines.push(RuleLine {
                number: format!("{}. ", index + 1),
                effect: match rule.effect {
                    AgentHttpRuleEffect::Allow => "allow",
                    AgentHttpRuleEffect::Deny => "deny",
                },
                conditions: Some(if !rule.has_conditions() {
                    "any arguments".to_owned()
                } else {
                    format_rule_conditions(&rule.arguments, rule.allow_fields.as_deref())
                }),
            });
        }
    }
    lines
}

/// Rule lines as printed, aligned so conditions start in one column.
fn render_rule_lines(lines: &[RuleLine], paint: Paint) -> Vec<String> {
    let effect_width = lines
        .iter()
        .filter(|line| line.conditions.is_some())
        .map(|line| line.effect.len())
        .max()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| line.render(effect_width, paint))
        .collect()
}

fn print_tool_details(
    server_name: &str,
    server: &AgentMcpServer,
    tool: &Value,
    warnings: &[String],
    json_format: bool,
) -> Result<()> {
    let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
    let (allowed, reason) = tool_decision(server, name);
    let schema = tool.get("inputSchema").unwrap_or(&Value::Null);
    let arguments = tool_arguments(schema);
    let rules = tool_rule_lines(server, name);
    let warnings = warnings
        .iter()
        .filter(|warning| warning.contains(&format!("'{name}'")))
        .cloned()
        .collect::<Vec<_>>();
    if json_format {
        println!(
            "{}",
            get_formatted_json_string(
                &json!({
                    "schema_version": 1,
                    "server": server_name,
                    "tool": name,
                    "description": tool.get("description"),
                    "allowed": allowed,
                    "reason": reason,
                    "arguments": arguments.iter().map(|argument| json!({
                        "path": argument.path,
                        "type": argument.kind,
                        "required": argument.required,
                        "values": argument.values,
                        "description": argument.description,
                    })).collect::<Vec<_>>(),
                    "rules": render_rule_lines(&rules, Paint(false)),
                    "warnings": warnings,
                    "input_schema": tool.get("inputSchema"),
                }),
                true,
            )?
        );
        return Ok(());
    }
    println!("MCP server: {server_name}");
    match tool.get("description").and_then(Value::as_str) {
        Some(description) => println!("Tool: {name} - {}", first_sentence(description)),
        None => println!("Tool: {name}"),
    }
    let paint = Paint::new();
    let decision = if allowed {
        paint.allowed("ALLOWED")
    } else {
        paint.denied("DENIED")
    };
    println!("Decision: {decision} {}", paint.dim(&format!("({reason})")));
    println!();
    if arguments.is_empty() {
        println!("{} none declared", paint.header("Arguments:"));
    } else {
        print_argument_table(&arguments, &rule_fields(server, name), paint);
    }
    println!();
    if rules.is_empty() {
        println!("{} none name this tool", paint.header("Profile rules:"));
    } else {
        println!("{}", paint.header("Profile rules:"));
        for line in render_rule_lines(&rules, paint) {
            println!("  {line}");
        }
    }
    print_warnings(&warnings, paint);
    Ok(())
}

/// Argument paths that tool rules naming `name` put conditions on.
fn rule_fields(server: &AgentMcpServer, name: &str) -> HashSet<String> {
    server
        .tool_rules
        .iter()
        .filter(|rule| rule.tools.iter().any(|tool| tool == "*" || tool == name))
        .flat_map(|rule| rule.arguments.keys().cloned())
        .collect()
}

/// One line per argument, sized to the terminal: fields the profile's rules
/// use come first, and descriptions are cut to their first sentence. `--json`
/// keeps the full schema.
fn print_argument_table(arguments: &[ToolArgument], rule_fields: &HashSet<String>, paint: Paint) {
    let mut rows = arguments.iter().collect::<Vec<_>>();
    rows.sort_by_key(|argument| !rule_fields.contains(&argument.path));
    let any_required = rows.iter().any(|argument| argument.required);
    let path_width = rows
        .iter()
        .map(|argument| argument.path.chars().count())
        .max()
        .unwrap_or(0);
    let kind_width = rows
        .iter()
        .map(|argument| argument.kind.chars().count())
        .max()
        .unwrap_or(0);
    let (terminal_width, _) = crate::utils::term_size::get_terminal_size();
    let fixed = 4 + path_width + 2 + kind_width + 2 + if any_required { 10 } else { 0 };
    let detail_width = terminal_width.saturating_sub(fixed);
    if rule_fields.is_empty() {
        println!("{}", paint.header(&format!("Arguments ({}):", rows.len())));
    } else {
        println!(
            "{} {}",
            paint.header(&format!("Arguments ({}):", rows.len())),
            paint.dim("* = used by a profile rule")
        );
    }
    for argument in rows {
        let mut detail = argument
            .description
            .as_deref()
            .map(first_sentence)
            .unwrap_or_default()
            .to_owned();
        if !argument.values.is_empty() {
            if !detail.is_empty() {
                detail.push(' ');
            }
            detail.push_str(&format!("[{}]", argument.values.join(", ")));
        }
        let used = rule_fields.contains(&argument.path);
        let marker = if used { "*" } else { " " };
        let required = match (any_required, argument.required) {
            (false, _) => String::new(),
            (true, true) => "required  ".to_owned(),
            (true, false) => " ".repeat(10),
        };
        // Pad before coloring: escape codes would otherwise count as width.
        let path = format!("{:path_width$}", argument.path);
        let kind = format!("{:kind_width$}", argument.kind);
        let detail = truncate(&detail, detail_width);
        let line = format!(
            "  {} {}  {}  {required}{detail}",
            paint.key(marker, used),
            paint.key(&path, used),
            paint.kind(&kind),
        );
        println!("{}", line.trim_end());
    }
    println!(
        "{}",
        paint.dim("Descriptions are shortened; --json prints the full input schema.")
    );
}

/// Text up to and including the first sentence-ending period, or the first
/// line when there is none.
fn first_sentence(text: &str) -> &str {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    match line.find(". ") {
        Some(end) => &line[..=end],
        None => line,
    }
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width < 2 {
        return String::new();
    }
    let mut cut = text.chars().take(width - 1).collect::<String>();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

fn print_warnings(warnings: &[String], paint: Paint) {
    if warnings.is_empty() {
        return;
    }
    println!();
    println!("{}", paint.header("Warnings:"));
    for warning in warnings {
        println!("- {}", paint.warning(warning));
    }
}

/// Terminal styles matching `agent profiles show`: cyan section headers, blue
/// keys and dimmed secondary text, plus magenta argument types. Plain text when
/// color is disabled.
#[derive(Clone, Copy)]
pub(crate) struct Paint(bool);

impl Paint {
    pub(crate) fn new() -> Self {
        Self(is_color_enabled(true))
    }

    pub(crate) fn header(self, text: &str) -> String {
        self.style(text, |text| text.cyan().bold().to_string())
    }

    fn key(self, text: &str, emphasized: bool) -> String {
        if emphasized {
            self.style(text, |text| text.blue().bold().to_string())
        } else {
            self.style(text, |text| text.blue().to_string())
        }
    }

    fn kind(self, text: &str) -> String {
        self.style(text, |text| text.magenta().to_string())
    }

    pub(crate) fn dim(self, text: &str) -> String {
        self.style(text, |text| text.bright_black().to_string())
    }

    pub(crate) fn allowed(self, text: &str) -> String {
        self.style(text, |text| text.green().bold().to_string())
    }

    pub(crate) fn denied(self, text: &str) -> String {
        self.style(text, |text| text.red().bold().to_string())
    }

    pub(crate) fn allowed_soft(self, text: &str) -> String {
        self.style(text, |text| text.green().to_string())
    }

    pub(crate) fn denied_soft(self, text: &str) -> String {
        self.style(text, |text| text.red().to_string())
    }

    fn warning(self, text: &str) -> String {
        self.style(text, |text| text.yellow().to_string())
    }

    fn style(self, text: &str, paint: impl Fn(&str) -> String) -> String {
        if self.0 {
            paint(text)
        } else {
            text.to_owned()
        }
    }
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
    let server = mcp_server(&profile, &command.server, false)?.clone();
    let url = mcp_url(&server)?;
    let mut spinner = SpinnerGuard::new(!silent);
    let inspection = if command.remote {
        remote_proxied_client(&profile, &server, api_key, false).await
    } else {
        proxied_client(&profile, &server, api_key, false).await
    };
    let (_proxy, client, auth) = match inspection {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
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
        let labels = names
            .iter()
            .map(|name| configure_item_label(&server, name))
            .collect::<Vec<_>>();
        let selected = MultiSelect::with_theme(&McpMultiSelectTheme::default())
            .with_prompt("Select tools to allow with any arguments")
            .items(&labels)
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
    // allow_tools allows every call of a tool, so selecting a tool that a
    // conditioned tool rule covers would quietly stop enforcing the rule.
    let shadowed = crate::models::agent::shadowed_tool_conditions(&selected, &server.tool_rules);
    for (rule, tool) in &shadowed {
        eprintln!(
            "Warning: tool rule {rule}: {}",
            crate::handlers::agent::validate::shadowed_condition_message(tool)
        );
    }
    if !Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Save {summary} to the profile?"))
        .default(shadowed.is_empty())
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

/// A picker item: the tool name, plus a note when tool rules already decide
/// its calls, so it isn't selected (and its conditions bypassed) by mistake.
fn configure_item_label(server: &AgentMcpServer, name: &str) -> String {
    let rules = server
        .tool_rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.tools.iter().any(|tool| tool == name))
        .map(|(index, rule)| {
            let effect = match rule.effect {
                AgentHttpRuleEffect::Allow => "allow",
                AgentHttpRuleEffect::Deny => "deny",
            };
            let conditions = if !rule.has_conditions() {
                ""
            } else {
                " with conditions"
            };
            format!("rule {} {effect}{conditions}", index + 1)
        })
        .collect::<Vec<_>>();
    if rules.is_empty() {
        name.to_owned()
    } else {
        format!("{name}  (tool {})", rules.join(", "))
    }
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
    let server = mcp_server(&profile, &command.server, json_format)?;
    let url = mcp_url(server)?;
    let mut spinner = SpinnerGuard::new(!silent);
    let inspection = if command.remote {
        remote_proxied_client(&profile, server, api_key, json_format).await
    } else {
        proxied_client(&profile, server, api_key, json_format).await
    };
    let (_proxy, client, auth) = match inspection {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
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
    let mcp_rules = server_mcp_rules(server, &host, &path);
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
        allow_network_listeners: false,
        egress_hosts_configured: profile.egress_hosts.is_some(),
        strict_deny: true,
        mcp_rules: mcp_rules.clone(),
        backend: profile.sandbox.backend,
        sandbox_image: profile.sandbox.image.clone(),
        sandbox_dockerfile: profile.sandbox.dockerfile.clone(),
        sandbox_memory: profile.sandbox.memory.clone(),
        sandbox_cpus: profile.sandbox.cpus.clone(),
        worktree: profile.workspace.worktree,
        worktree_resume: None,
        sandbox_isolated_paths: profile.sandbox.isolated_paths.clone(),
        workspace_extra_paths: profile.workspace.extra_paths.clone(),
        config_env: Default::default(),
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

/// The proxy rules for one MCP server: `allow_tools`, `deny_tools` and every
/// tool rule with its argument conditions and allowed fields.
pub(crate) fn server_mcp_rules(
    server: &AgentMcpServer,
    host: &str,
    path: &str,
) -> Vec<AgentMcpRule> {
    let rule = |effect: AgentHttpRuleEffect, tools: &[String]| AgentMcpRule {
        effect,
        hosts: vec![host.to_owned()],
        paths: vec![path.to_owned()],
        tools: tools.to_vec(),
        arguments: Default::default(),
        allow_fields: None,
    };
    let mut rules = vec![rule(AgentHttpRuleEffect::Allow, &server.allow_tools)];
    if !server.deny_tools.is_empty() {
        rules.push(rule(AgentHttpRuleEffect::Deny, &server.deny_tools));
    }
    for tool_rule in &server.tool_rules {
        rules.push(AgentMcpRule {
            arguments: tool_rule.arguments.clone(),
            allow_fields: tool_rule.allow_fields.clone(),
            ..rule(tool_rule.effect.clone(), &tool_rule.tools)
        });
    }
    rules
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
    let (host, path) = mcp_endpoint_parts(server)?;
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
    let mcp_rules = server_mcp_rules(server, &host, &path);
    let request = crate::api::remote_proxy::RemoteProxySessionRequest {
        api_key: api_key.to_owned(),
        project_identifier: profile.secrets.project.clone(),
        environment_identifier: profile.secrets.environment.clone(),
        egress_hosts: profile.egress_hosts.clone().unwrap_or_default(),
        deny_hosts: profile.deny_hosts.clone().unwrap_or_default(),
        bindings: bindings.clone(),
        mcp_rules: mcp_rules
            .iter()
            .cloned()
            .map(crate::api::remote_proxy::RemoteMcpRule::from)
            .collect(),
        agent_type: None,
        session_purpose: Some("mcp_inspection".to_owned()),
        previous_session_token: None,
        routing_mode: None,
    };
    let session = crate::api::remote_proxy::create_session(&request, json_format).await?;
    // Inspection talks to the MCP server with its credential, so that host
    // always goes through the Agent Proxy whatever mode the control plane picks.
    let routing = crate::handlers::run::routing::resolve_session_routing(
        &session,
        &bindings,
        std::slice::from_ref(&host),
    )?;
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
        allow_network_listeners: false,
        egress_hosts_configured: profile.egress_hosts.is_some(),
        strict_deny: true,
        mcp_rules,
        backend: profile.sandbox.backend,
        sandbox_image: profile.sandbox.image.clone(),
        sandbox_dockerfile: profile.sandbox.dockerfile.clone(),
        sandbox_memory: profile.sandbox.memory.clone(),
        sandbox_cpus: profile.sandbox.cpus.clone(),
        worktree: profile.workspace.worktree,
        worktree_resume: None,
        sandbox_isolated_paths: profile.sandbox.isolated_paths.clone(),
        workspace_extra_paths: profile.workspace.extra_paths.clone(),
        config_env: Default::default(),
    };
    let proxy = Proxy::start_remote_with_port(
        RemoteProxyConfig {
            proxy_url: session.proxy_url,
            session: Arc::new(RwLock::new(state)),
            placeholders,
            child_env,
            protocol,
            ca_file,
            routing: Arc::new(RwLock::new(routing)),
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
    let source_name = binding_source_name(name, binding);
    let local_value = local_binding_value(profile.file.as_deref(), source_name)?;
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
            vec![source_name.to_owned()],
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
            .find(|secret| secret.name == source_name)
            .map(|secret| secret.value)
            .context(format!(
                "MCP binding '{name}' was not found in the configured project/environment"
            ))?
    } else {
        return Err(anyhow::anyhow!(format!(
            "MCP binding '{name}' is not available from any configured source"
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

fn binding_source_name<'a>(name: &'a str, binding: &'a AgentBindingProfile) -> &'a str {
    binding.from.as_deref().unwrap_or(name)
}

fn local_binding_value(file: Option<&str>, source_name: &str) -> Result<Option<String>> {
    let file_value = file
        .map(|file| {
            read_secrets_from_file(Path::new(file), &SecretsFileFormat::Dotenv)
                .with_context(|| format!("Could not read configured MCP secret file '{file}'."))
                .map(|items| {
                    items
                        .into_iter()
                        .find(|item| item.name == source_name)
                        .map(|item| item.value)
                })
        })
        .transpose()?
        .flatten();
    Ok(file_value)
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
    let server = mcp_server(&profile, &command.server, json_format)?;
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
    let names_tool = |tools: &[String]| tools.iter().any(|tool| tool == "*" || tool == name);
    let tool_rules = |effect: AgentHttpRuleEffect, conditioned: bool| {
        server.tool_rules.iter().any(|rule| {
            rule.effect == effect && rule.has_conditions() == conditioned && names_tool(&rule.tools)
        })
    };
    if names_tool(&server.deny_tools) {
        return (false, "matched deny_tools");
    }
    if tool_rules(AgentHttpRuleEffect::Deny, false) {
        return (false, "matched a deny tool rule");
    }
    let shadowed = server
        .shadowed_tool_conditions()
        .iter()
        .any(|(_, tool)| tool == "*" || tool == name);
    let (allowed, reason) = if shadowed {
        (
            true,
            "allowed with any arguments; tool rule conditions have no effect because allow_tools or a rule without arguments also allows it",
        )
    } else if names_tool(&server.allow_tools) {
        (true, "matched allow_tools")
    } else if tool_rules(AgentHttpRuleEffect::Allow, false) {
        (true, "matched an allow tool rule")
    } else if tool_rules(AgentHttpRuleEffect::Allow, true) {
        (
            true,
            "allowed only when an allow tool rule's argument conditions hold",
        )
    } else if server.allow_tools.is_empty() && server.tool_rules.is_empty() {
        (false, "allow_tools is empty; no tools are allowed")
    } else if server.tool_rules.is_empty() {
        (false, "not present in allow_tools")
    } else {
        (false, "not present in allow_tools or an allow tool rule")
    };
    if allowed && tool_rules(AgentHttpRuleEffect::Deny, true) {
        let reason = if reason.starts_with("allowed only") {
            "allowed only when an allow tool rule's argument conditions hold, except calls whose arguments match a deny tool rule"
        } else {
            "allowed, except calls whose arguments match a deny tool rule"
        };
        return (true, reason);
    }
    (allowed, reason)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs};

    use super::{
        agent_input_error, binding_source_name, configure_item_label, first_sentence,
        format_conditions, local_binding_value, mcp_tools_page, render_rule_lines,
        replace_mcp_allow_tools, tool_arguments, tool_decision, tool_rule_lines,
        tool_rule_warnings, truncate, Paint,
    };
    use crate::models::{
        agent::{AgentBindingProfile, AgentMcpServer},
        validation::AgentSessionInputValidationError,
    };
    use uuid::Uuid;

    fn server(allow_tools: &[&str], deny_tools: &[&str]) -> AgentMcpServer {
        AgentMcpServer {
            url: "https://mcp.example.com/mcp".to_owned(),
            binding: None,
            header: None,
            value_template: None,
            allow_tools: allow_tools.iter().map(|tool| (*tool).to_owned()).collect(),
            deny_tools: deny_tools.iter().map(|tool| (*tool).to_owned()).collect(),
            tool_rules: Vec::new(),
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
    fn tool_decision_reports_tool_rules() {
        let mut server = server(&["search"], &[]);
        server.tool_rules = toml::from_str::<std::collections::HashMap<String, Vec<_>>>(
            r#"
rules = [
  { effect = "allow", tools = ["send_message"], arguments = { mode = "draft" } },
  { effect = "deny", tools = ["search"], arguments = { scope = "all" } },
  { effect = "deny", tools = ["delete_message"] },
]
"#,
        )
        .unwrap()
        .remove("rules")
        .unwrap();
        assert_eq!(
            tool_decision(&server, "send_message"),
            (
                true,
                "allowed only when an allow tool rule's argument conditions hold"
            )
        );
        assert_eq!(
            tool_decision(&server, "search"),
            (
                true,
                "allowed, except calls whose arguments match a deny tool rule"
            )
        );
        assert_eq!(
            tool_decision(&server, "delete_message"),
            (false, "matched a deny tool rule")
        );
        assert_eq!(
            tool_decision(&server, "other"),
            (false, "not present in allow_tools or an allow tool rule")
        );
    }

    fn save_issue_tool() -> serde_json::Value {
        serde_json::json!({
            "name": "save_issue",
            "inputSchema": {
                "type": "object",
                "required": ["title"],
                "properties": {
                    "id": {"type": "string", "description": "Issue ID.\nUpdates when set."},
                    "title": {"type": "string"},
                    "priority": {"type": ["number", "null"], "enum": [0, 1, 2, 3, 4]},
                    "labels": {"type": "array", "items": {"type": "string"}},
                    "team": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                    "project": {
                        "type": "object",
                        "properties": {"id": {"type": "string"}}
                    }
                }
            }
        })
    }

    #[test]
    fn tool_arguments_flatten_the_input_schema_into_rule_paths() {
        let arguments = tool_arguments(&save_issue_tool()["inputSchema"]);
        let summary = arguments
            .iter()
            .map(|argument| {
                (
                    argument.path.as_str(),
                    argument.kind.as_str(),
                    argument.required,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                ("id", "string", false),
                ("labels", "string[]", false),
                ("priority", "number", false),
                ("project", "object", false),
                ("project.id", "string", false),
                ("team", "string", false),
                ("title", "string", true),
            ]
        );
        assert_eq!(arguments[0].description.as_deref(), Some("Issue ID."));
        assert_eq!(arguments[2].values, vec!["0", "1", "2", "3", "4"]);
    }

    #[test]
    fn tool_rule_warnings_catch_unknown_tools_and_fields() {
        let mut server = server(&["list_issues", "save_issue"], &[]);
        server.tool_rules = toml::from_str::<HashMap<String, Vec<_>>>(
            r#"
rules = [
  { effect = "allow", tools = ["save_issue"], arguments = { team = "ENG", "project.id" = "p", teem = "ENG" } },
  { effect = "deny", tools = ["save_isue"], arguments = { priority = 1 } },
]
"#,
        )
        .unwrap()
        .remove("rules")
        .unwrap();
        assert_eq!(
            tool_rule_warnings(&server, &[save_issue_tool()]),
            vec![
                "allow_tools names 'list_issues', which the server does not offer.",
                "Tool rule 1: Argument conditions for 'save_issue' have no effect: allow_tools or a tool rule without arguments already allows every call. Remove 'save_issue' from those to enforce the conditions.",
                "Tool rule 1: arguments.teem is not an argument of 'save_issue'.",
                "Tool rule 2 names 'save_isue', which the server does not offer.",
            ]
        );
    }

    #[test]
    fn tool_rule_lines_show_conditions_in_profile_syntax() {
        let mut server = server(&[], &[]);
        server.tool_rules = toml::from_str::<HashMap<String, Vec<_>>>(
            r#"
rules = [
  { effect = "allow", tools = ["save_issue"], arguments = { team = ["ENG", "OPS"], id = { absent = true }, estimate = { min = 1, max = 5 } } },
  { effect = "deny", tools = ["*"], arguments = { priority = 1 } },
  { effect = "allow", tools = ["save_issue"] },
]
"#,
        )
        .unwrap()
        .remove("rules")
        .unwrap();
        assert_eq!(
            render_rule_lines(&tool_rule_lines(&server, "save_issue"), Paint(false)),
            vec![
                r#"1. allow: estimate = { min = 1, max = 5 }, id = { absent = true }, team = ["ENG", "OPS"]"#,
                "2. deny:  priority = 1",
                "3. allow: any arguments",
            ]
        );
        assert_eq!(format_conditions(&Default::default()), "");
    }

    #[test]
    fn paint_is_plain_without_color_and_styled_with_it() {
        let plain = Paint(false);
        assert_eq!(plain.header("Arguments:"), "Arguments:");
        assert_eq!(plain.key("team", true), "team");
        assert_eq!(plain.warning("check this"), "check this");
        let colored = Paint(true);
        assert!(colored.header("Arguments:").contains("\u{1b}["));
        assert!(colored.denied("DENIED").contains("DENIED"));
    }

    #[test]
    fn argument_descriptions_are_cut_to_a_sentence_and_width() {
        assert_eq!(
            first_sentence(
                "Only for updating an existing issue. Pass the issue ID (e.g., LIN-123)."
            ),
            "Only for updating an existing issue."
        );
        assert_eq!(first_sentence("0=None, 1=Urgent\nmore"), "0=None, 1=Urgent");
        assert_eq!(first_sentence("Version 1.2 is fine"), "Version 1.2 is fine");
        assert_eq!(truncate("Team name or ID", 40), "Team name or ID");
        assert_eq!(truncate("Team name or ID", 10), "Team name…");
        assert_eq!(truncate("Team name", 1), "");
    }

    #[test]
    fn configure_labels_tools_that_tool_rules_decide() {
        let mut server = server(&[], &[]);
        server.tool_rules = toml::from_str::<HashMap<String, Vec<_>>>(
            r#"
rules = [
  { effect = "allow", tools = ["save_issue"], arguments = { team = "ENG" } },
  { effect = "deny", tools = ["save_issue"], arguments = { priority = 1 } },
  { effect = "deny", tools = ["delete_issue"] },
]
"#,
        )
        .unwrap()
        .remove("rules")
        .unwrap();
        assert_eq!(
            configure_item_label(&server, "save_issue"),
            "save_issue  (tool rule 1 allow with conditions, rule 2 deny with conditions)"
        );
        assert_eq!(
            configure_item_label(&server, "delete_issue"),
            "delete_issue  (tool rule 3 deny)"
        );
        assert_eq!(configure_item_label(&server, "list_issues"), "list_issues");
    }

    #[test]
    fn tool_decision_reports_shadowed_conditions() {
        let mut server = server(&["*"], &[]);
        server.tool_rules = toml::from_str::<HashMap<String, Vec<_>>>(
            r#"rules = [{ effect = "allow", tools = ["save_issue"], arguments = { team = "ENG" } }]"#,
        )
        .unwrap()
        .remove("rules")
        .unwrap();
        let (allowed, reason) = tool_decision(&server, "save_issue");
        assert!(allowed);
        assert!(reason.contains("conditions have no effect"), "{reason}");
        assert!(tool_rule_warnings(&server, &[save_issue_tool()])
            .iter()
            .any(|warning| warning.contains("'save_issue' have no effect")));
    }

    #[test]
    fn not_found_errors_are_json_under_json_output() {
        let error = agent_input_error(
            AgentSessionInputValidationError::McpToolNotFound {
                server: "linear".to_owned(),
                tool: "list_projectss".to_owned(),
            },
            true,
        );
        let value: serde_json::Value = serde_json::from_str(&error.to_string()).unwrap();
        assert_eq!(value["error"]["type"], "input_validation_error");
        assert_eq!(
            value["error"]["message"],
            "MCP tool was not found on the server."
        );
        assert!(value["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("'linear' does not offer a tool named 'list_projectss'"));

        let text = agent_input_error(
            AgentSessionInputValidationError::McpServerNotFound {
                server: "lineer".to_owned(),
            },
            false,
        )
        .to_string();
        assert!(text.contains("MCP server was not found in the profile."));
        assert!(text.contains("[mcp_servers.lineer]"));
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
    fn local_profile_file_overrides_shell_binding_value() {
        let name = format!("STASHBASE_MCP_TEST_{}", Uuid::new_v4().simple());
        let path = std::env::temp_dir().join(format!("{name}.env"));
        fs::write(&path, format!("{name}=from-file\n")).unwrap();
        std::env::set_var(&name, "from-shell");

        assert_eq!(
            local_binding_value(path.to_str(), &name)
                .unwrap()
                .as_deref(),
            Some("from-file")
        );

        std::env::remove_var(&name);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn mcp_binding_reads_the_canonical_source_not_the_child_env_name() {
        let binding = AgentBindingProfile {
            hosts: Vec::new(),
            rules: Vec::new(),
            from: None,
            env: Some("GITHUB_PAT_TOKEN".to_owned()),
            placeholder: None,
            header: None,
            value_template: None,
        };

        assert_eq!(
            binding_source_name("GITHUB_TOKEN", &binding),
            "GITHUB_TOKEN"
        );
    }

    #[test]
    fn configured_missing_profile_file_does_not_fall_back_to_shell() {
        let name = format!("STASHBASE_MCP_TEST_{}", Uuid::new_v4().simple());
        let path = std::env::temp_dir().join(format!("{name}.env"));
        std::env::set_var(&name, "from-shell");

        let error = local_binding_value(path.to_str(), &name).unwrap_err();

        assert!(error
            .to_string()
            .contains("Could not read configured MCP secret file"));
        std::env::remove_var(&name);
    }

    #[test]
    fn configured_profile_file_without_source_does_not_fall_back_to_shell() {
        let name = format!("STASHBASE_MCP_TEST_{}", Uuid::new_v4().simple());
        let path = std::env::temp_dir().join(format!("{name}.env"));
        fs::write(&path, "OTHER=value\n").unwrap();
        std::env::set_var(&name, "from-shell");

        assert_eq!(local_binding_value(path.to_str(), &name).unwrap(), None);

        std::env::remove_var(&name);
        fs::remove_file(path).unwrap();
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
