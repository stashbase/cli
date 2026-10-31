//! stdio MCP servers that `agent run` starts on the agent's behalf. A stdio
//! server talks to its client over pipes, which the Agent Proxy never sees,
//! so a server the agent spawned itself could not be held to the profile's
//! tool rules. Instead the run starts each server, sandboxed like the agent,
//! and the agent reaches it through `stashbase agent mcp connect <name>`: an
//! upgraded connection to the proxy, which checks every line against the
//! same rules the HTTP path enforces before passing it on.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};

use super::proxy::{authorize_mcp_request, filter_mcp_tools_list_response, McpDenial, ProxyPolicy};
use crate::models::agent::STDIO_MCP_HOST;

/// Routes under the proxy address; the server name follows.
pub const MCP_BROKER_PATH: &str = "/__stashbase/mcp/";
pub const MCP_BROKER_URL_ENV: &str = "STASHBASE_MCP_BROKER_URL";
pub const MCP_BROKER_TOKEN_ENV: &str = "STASHBASE_MCP_BROKER_TOKEN";
/// The `Upgrade` protocol `agent mcp connect` asks for.
pub const MCP_UPGRADE_PROTOCOL: &str = "stashbase-mcp";
/// JSON-RPC error code for a call the profile refused (implementation-defined range).
const POLICY_DENIED_CODE: i64 = -32001;
/// JSON-RPC error code for a request the server exited without answering.
const SERVER_EXITED_CODE: i64 = -32002;
/// No single MCP message is anywhere near this; a line that is would only
/// make the broker buffer without bound.
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// How long a server may keep answering after the agent stops sending.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// A stdio server as the profile declares it, before the run knows the
/// agent's environment.
#[derive(Debug, Clone, PartialEq)]
pub struct StdioMcpServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, StdioEnvValue>,
    /// Relative to the run's workdir.
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StdioEnvValue {
    Plain(String),
    /// The value the agent's own environment has for this variable: a
    /// binding's placeholder, so the server never holds the real secret.
    FromChildEnv(String),
}

impl StdioMcpServerConfig {
    /// The server's environment: the agent's, then the profile's `env`.
    pub fn resolve_env(
        &self,
        child_env: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut env = child_env.clone();
        for (name, value) in &self.env {
            let value = match value {
                StdioEnvValue::Plain(value) => value.clone(),
                StdioEnvValue::FromChildEnv(source) => child_env
                    .get(source)
                    .cloned()
                    .with_context(|| format!("{name}: the agent environment has no {source}"))?,
            };
            env.insert(name.clone(), value);
        }
        Ok(env)
    }
}

/// Everything needed to start one server process: the sandbox launcher is
/// already part of `program` and `args`.
#[derive(Debug, Clone)]
pub struct StdioServerLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub env_removals: Vec<String>,
    pub cwd: PathBuf,
    /// Where the server's stderr goes. Discarded when `None`.
    pub stderr_log: Option<PathBuf>,
    /// For a server in its own container: the index in `args` of the
    /// container name, which each connection suffixes to make unique, and
    /// which is removed when the server stops.
    pub container_name_arg: Option<usize>,
}

pub struct McpBroker {
    pub(super) token: String,
    pub(super) servers: HashMap<String, StdioServerLaunch>,
}

impl McpBroker {
    pub fn new(servers: HashMap<String, StdioServerLaunch>) -> Self {
        Self {
            token: uuid::Uuid::new_v4().to_string(),
            servers,
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

/// The pseudo-endpoint path a server's rules are compiled under.
pub fn rule_path(name: &str) -> String {
    format!("/{name}")
}

/// What to do with one line the agent sent.
#[derive(Debug, PartialEq)]
pub enum ClientLine {
    Forward,
    /// Not forwarded; `reply` goes back to the agent.
    Refuse {
        reply: Vec<u8>,
        tool: Option<String>,
        denial: McpDenial,
    },
}

/// The per-connection view of the JSON-RPC stream: which requests asked for
/// `tools/list`, so their responses can be filtered on the way back.
pub struct LineFilter<'a> {
    policy: &'a ProxyPolicy,
    path: String,
    pending_lists: HashSet<String>,
}

impl<'a> LineFilter<'a> {
    pub fn new(policy: &'a ProxyPolicy, name: &str) -> Self {
        Self {
            policy,
            path: rule_path(name),
            pending_lists: HashSet::new(),
        }
    }

    pub fn client_line(&mut self, line: &[u8]) -> ClientLine {
        let value = serde_json::from_slice::<Value>(line).ok();
        if let Err(denial) =
            authorize_mcp_request(line, self.policy, Some(STDIO_MCP_HOST), &self.path)
        {
            let id = value
                .as_ref()
                .and_then(|value| value.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            let tool = value
                .as_ref()
                .and_then(|value| value.pointer("/params/name"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let reply = serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": POLICY_DENIED_CODE, "message": denial.message },
            }))
            .expect("a JSON-RPC error always serializes");
            return ClientLine::Refuse {
                reply,
                tool,
                denial,
            };
        }
        if let Some(value) = &value {
            if value.get("method").and_then(Value::as_str) == Some("tools/list") {
                if let Some(id) = value.get("id") {
                    self.pending_lists.insert(id.to_string());
                }
            }
        }
        ClientLine::Forward
    }

    /// The line to send the agent: a `tools/list` result without the tools
    /// the profile denies, anything else unchanged.
    pub fn server_line(&mut self, line: &[u8]) -> Vec<u8> {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return line.to_vec();
        };
        let is_response = value.get("method").is_none();
        let Some(id) = value.get("id").filter(|_| is_response) else {
            return line.to_vec();
        };
        if !self.pending_lists.remove(&id.to_string()) {
            return line.to_vec();
        }
        match filter_mcp_tools_list_response(line, self.policy, Some(STDIO_MCP_HOST), &self.path) {
            Ok(filtered) => filtered.to_vec(),
            // A result the filter can't read can't be shown safely either.
            Err(_) => serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [] },
            }))
            .expect("an empty tools/list result always serializes"),
        }
    }
}

/// One audit decision the relay reports to the proxy.
pub enum RelayEvent<'a> {
    ToolCalled {
        tool: &'a str,
    },
    ToolRefused {
        tool: Option<&'a str>,
        denial: &'a McpDenial,
    },
}

/// Starts the server and relays newline-delimited JSON-RPC between `client`
/// and it until either side closes. The server is stopped on return.
pub async fn serve<IO>(
    client: IO,
    name: &str,
    launch: &StdioServerLaunch,
    policy: &ProxyPolicy,
    mut on_event: impl FnMut(RelayEvent<'_>, Duration),
) -> Result<()>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let (mut child, container) =
        spawn(launch).with_context(|| format!("failed to start MCP server '{name}'"))?;
    let result = relay(
        client,
        &mut child,
        name,
        launch.stderr_log.as_deref(),
        policy,
        &mut on_event,
    )
    .await;
    stop(&mut child, container.as_deref()).await;
    result
}

/// The server process, and its container's name when it runs in one.
fn spawn(launch: &StdioServerLaunch) -> Result<(Child, Option<String>)> {
    let mut args = launch.args.clone();
    let container = match launch.container_name_arg {
        Some(index) => {
            let name = args
                .get_mut(index)
                .context("container name argument is missing")?;
            name.push_str(&format!(
                "-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ));
            Some(name.clone())
        }
        None => None,
    };
    let mut command = Command::new(&launch.program);
    command
        .args(&args)
        .current_dir(&launch.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    for name in &launch.env_removals {
        command.env_remove(name);
    }
    command.envs(&launch.env);
    // A group of its own, so stopping it reaches whatever it started (npx,
    // the sandbox launcher's children) and Ctrl-C in the agent's terminal
    // doesn't reach it.
    #[cfg(unix)]
    command.process_group(0);
    match &launch.stderr_log {
        Some(path) => {
            if let Some(dir) = path.parent() {
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(dir)?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            command.stderr(options.open(path)?);
        }
        None => {
            command.stderr(Stdio::null());
        }
    }
    Ok((command.spawn()?, container))
}

async fn relay<IO>(
    client: IO,
    child: &mut Child,
    name: &str,
    stderr_log: Option<&std::path::Path>,
    policy: &ProxyPolicy,
    on_event: &mut impl FnMut(RelayEvent<'_>, Duration),
) -> Result<()>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let mut server_in = child.stdin.take().context("server stdin is not piped")?;
    let server_out = child.stdout.take().context("server stdout is not piped")?;
    let (client_read, mut client_write) = tokio::io::split(client);
    let mut client_lines = BufReader::new(client_read);
    let mut server_lines = BufReader::new(server_out);
    let mut filter = LineFilter::new(policy, name);
    // Both directions write to the agent; refusals jump the queue only in
    // the sense that they're written as soon as they're decided.
    let (to_client, mut to_client_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut client_buffer = Vec::new();
    let mut server_buffer = Vec::new();
    let mut client_open = true;
    let mut unanswered = HashSet::<String>::new();
    // Once the agent has sent its last message, how long the server gets to
    // answer what it already received.
    let drain = tokio::time::sleep(Duration::MAX);
    tokio::pin!(drain);
    loop {
        tokio::select! {
            read = read_line(&mut client_lines, &mut client_buffer), if client_open => {
                if read? == 0 {
                    // The agent is done sending. Close the server's stdin so
                    // it can finish and exit on its own, and keep relaying
                    // its answers until it does.
                    client_open = false;
                    let _ = server_in.shutdown().await;
                    drain.as_mut().reset(tokio::time::Instant::now() + DRAIN_TIMEOUT);
                    continue;
                }
                let started = Instant::now();
                let line = trim_newline(&client_buffer);
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match filter.client_line(line) {
                    ClientLine::Forward => {
                        if let Some(tool) = called_tool(line) {
                            on_event(RelayEvent::ToolCalled { tool: &tool }, started.elapsed());
                        }
                        if let Some(id) = request_id(line) {
                            unanswered.insert(id);
                        }
                        let written = async {
                            server_in.write_all(line).await?;
                            server_in.write_all(b"\n").await?;
                            server_in.flush().await
                        };
                        if written.await.is_err() {
                            // The server is gone; its stdout's end reports it.
                            client_open = false;
                        }
                    }
                    ClientLine::Refuse { reply, tool, denial } => {
                        on_event(
                            RelayEvent::ToolRefused { tool: tool.as_deref(), denial: &denial },
                            started.elapsed(),
                        );
                        let _ = to_client.send(reply);
                    }
                }
            }
            read = read_line(&mut server_lines, &mut server_buffer) => {
                if read? == 0 {
                    // Requests the server never answered would otherwise
                    // leave the agent waiting until its own timeout.
                    for id in unanswered.drain() {
                        let _ = to_client.send(exited_reply(&id, name, stderr_log));
                    }
                    break;
                }
                let line = trim_newline(&server_buffer);
                if !line.is_empty() {
                    if let Some(id) = response_id(line) {
                        unanswered.remove(&id);
                    }
                    let _ = to_client.send(filter.server_line(line));
                }
            }
            Some(mut message) = to_client_rx.recv() => {
                message.push(b'\n');
                client_write.write_all(&message).await?;
                client_write.flush().await?;
            }
            () = &mut drain => break,
        }
    }
    // Deliver whatever was decided before the loop ended.
    to_client_rx.close();
    while let Ok(mut message) = to_client_rx.try_recv() {
        message.push(b'\n');
        client_write.write_all(&message).await?;
    }
    let _ = client_write.shutdown().await;
    Ok(())
}

/// Reads one line into `buffer`, replacing what it held. Fails on a line
/// longer than `MAX_LINE_BYTES`.
async fn read_line<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
) -> Result<usize> {
    buffer.clear();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(buffer.len());
        }
        let (taken, done) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        buffer.extend_from_slice(&available[..taken]);
        reader.consume(taken);
        if buffer.len() > MAX_LINE_BYTES {
            anyhow::bail!("MCP message exceeds {MAX_LINE_BYTES} bytes");
        }
        if done {
            return Ok(buffer.len());
        }
    }
}

fn trim_newline(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// The id of a request (a message with a method and an id), as JSON text.
fn request_id(line: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(line).ok()?;
    value.get("method")?;
    Some(value.get("id")?.to_string())
}

/// The id of a response (an id without a method), as JSON text.
fn response_id(line: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(line).ok()?;
    if value.get("method").is_some() {
        return None;
    }
    Some(value.get("id")?.to_string())
}

/// The answer to a request the server exited without answering.
fn exited_reply(id: &str, name: &str, stderr_log: Option<&std::path::Path>) -> Vec<u8> {
    let id = serde_json::from_str::<Value>(id).unwrap_or(Value::Null);
    let message = match stderr_log {
        Some(log) => format!(
            "MCP server '{name}' exited before answering; its output is in {}",
            log.display()
        ),
        None => format!("MCP server '{name}' exited before answering"),
    };
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": SERVER_EXITED_CODE, "message": message },
    }))
    .expect("a JSON-RPC error always serializes")
}

fn called_tool(line: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(line).ok()?;
    (value.get("method")?.as_str()? == "tools/call")
        .then(|| value.pointer("/params/name")?.as_str().map(str::to_owned))
        .flatten()
}

/// Asks the server's process group to exit, gives it a moment, then kills
/// whatever of the group is left. Killing `docker run` doesn't stop its
/// container, so a container is removed explicitly.
async fn stop(child: &mut Child, container: Option<&str>) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The group id is the server's pid (`process_group(0)`).
        let group = pid as libc::pid_t;
        unsafe { libc::killpg(group, libc::SIGTERM) };
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    let _ = child.kill().await;
    if let Some(container) = container {
        let _ = Command::new("docker")
            .args(["rm", "-f", container])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::agent::{
        AgentFieldCondition, AgentHttpRuleEffect, AgentMcpRule, AgentScalar,
    };

    fn policy() -> ProxyPolicy {
        let mut policy = ProxyPolicy::permissive();
        let rule = |effect, tools: &[&str]| AgentMcpRule {
            effect,
            hosts: vec![STDIO_MCP_HOST.to_owned()],
            paths: vec![rule_path("github")],
            tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
            arguments: Default::default(),
            allow_fields: None,
        };
        policy.mcp_rules = vec![
            rule(AgentHttpRuleEffect::Allow, &["get_issue", "list_issues"]),
            AgentMcpRule {
                arguments: BTreeMap::from([(
                    "owner".to_owned(),
                    AgentFieldCondition::Equals(AgentScalar::String("acme".to_owned())),
                )]),
                ..rule(AgentHttpRuleEffect::Allow, &["create_issue"])
            },
            rule(AgentHttpRuleEffect::Deny, &["list_issues"]),
        ];
        policy
    }

    fn call(id: u64, tool: &str, arguments: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        }))
        .unwrap()
    }

    fn refusal(decision: ClientLine) -> (Value, String) {
        match decision {
            ClientLine::Refuse { reply, denial, .. } => {
                (serde_json::from_slice(&reply).unwrap(), denial.reason)
            }
            ClientLine::Forward => panic!("expected a refusal"),
        }
    }

    #[test]
    fn allowed_calls_are_forwarded() {
        let policy = policy();
        let mut filter = LineFilter::new(&policy, "github");
        assert_eq!(
            filter.client_line(&call(1, "get_issue", serde_json::json!({}))),
            ClientLine::Forward
        );
        assert_eq!(
            filter.client_line(&call(
                2,
                "create_issue",
                serde_json::json!({ "owner": "acme" })
            )),
            ClientLine::Forward
        );
    }

    #[test]
    fn refused_calls_get_a_json_rpc_error_with_the_request_id() {
        let policy = policy();
        let mut filter = LineFilter::new(&policy, "github");
        let (reply, reason) =
            refusal(filter.client_line(&call(7, "delete_repo", serde_json::json!({}))));
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"]["code"], POLICY_DENIED_CODE);
        assert_eq!(
            reply["error"]["message"],
            "MCP tool is not allowed by the agent profile"
        );
        assert_eq!(reason, "tool_not_allowed");

        let (_, reason) =
            refusal(filter.client_line(&call(8, "list_issues", serde_json::json!({}))));
        assert_eq!(reason, "tool_denied");

        let (reply, reason) = refusal(filter.client_line(&call(
            9,
            "create_issue",
            serde_json::json!({ "owner": "evil" }),
        )));
        assert_eq!(reason, "allow_condition_failed:arguments.owner");
        assert!(reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("arguments.owner"));
    }

    #[test]
    fn malformed_lines_are_refused_like_the_http_path() {
        let policy = policy();
        let mut filter = LineFilter::new(&policy, "github");
        let (reply, reason) = refusal(filter.client_line(b"{not json"));
        assert_eq!(reply["id"], Value::Null);
        assert_eq!(reason, "invalid_json");
        let (_, reason) = refusal(
            filter.client_line(b"[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}]"),
        );
        assert_eq!(reason, "batch_request");
        let (_, reason) = refusal(filter.client_line(
            br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_issue","name":"delete_repo"}}"#,
        ));
        assert_eq!(reason, "duplicate_key");
    }

    #[test]
    fn other_methods_and_responses_pass_through() {
        let policy = policy();
        let mut filter = LineFilter::new(&policy, "github");
        for line in [
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            // A response to a server-initiated request (sampling, elicitation).
            br#"{"jsonrpc":"2.0","id":"s1","result":{}}"#,
        ] {
            assert_eq!(filter.client_line(line), ClientLine::Forward);
        }
    }

    #[test]
    fn tools_list_responses_hide_tools_the_profile_does_not_allow() {
        let policy = policy();
        let mut filter = LineFilter::new(&policy, "github");
        assert_eq!(
            filter.client_line(br#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#),
            ClientLine::Forward
        );
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": { "tools": [
                { "name": "get_issue" },
                { "name": "list_issues" },
                { "name": "create_issue" },
                { "name": "delete_repo" },
            ]},
        });
        let filtered: Value =
            serde_json::from_slice(&filter.server_line(&serde_json::to_vec(&response).unwrap()))
                .unwrap();
        let names = filtered["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["get_issue", "create_issue"]);
    }

    #[test]
    fn untracked_responses_and_server_requests_are_unchanged() {
        let policy = policy();
        let mut filter = LineFilter::new(&policy, "github");
        // Same shape as a tools/list result, but no tools/list asked for id 4.
        let untracked = br#"{"jsonrpc":"2.0","id":4,"result":{"tools":[{"name":"delete_repo"}]}}"#;
        assert_eq!(filter.server_line(untracked), untracked.to_vec());
        let request = br#"{"jsonrpc":"2.0","id":5,"method":"sampling/createMessage","params":{}}"#;
        assert_eq!(filter.server_line(request), request.to_vec());
    }

    #[test]
    fn server_env_adds_the_profile_env_over_the_agent_env() {
        let config = StdioMcpServerConfig {
            command: "npx".to_owned(),
            args: Vec::new(),
            env: BTreeMap::from([
                (
                    "GITHUB_PERSONAL_ACCESS_TOKEN".to_owned(),
                    StdioEnvValue::FromChildEnv("GH_TOKEN".to_owned()),
                ),
                (
                    "LOG_LEVEL".to_owned(),
                    StdioEnvValue::Plain("debug".to_owned()),
                ),
            ]),
            cwd: None,
        };
        let child_env = HashMap::from([
            ("GH_TOKEN".to_owned(), "**STASHBASE_GH_TOKEN**".to_owned()),
            ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:1".to_owned()),
        ]);
        let env = config.resolve_env(&child_env).unwrap();
        assert_eq!(
            env["GITHUB_PERSONAL_ACCESS_TOKEN"],
            "**STASHBASE_GH_TOKEN**"
        );
        assert_eq!(env["LOG_LEVEL"], "debug");
        assert_eq!(env["HTTPS_PROXY"], "http://127.0.0.1:1");

        let missing = StdioMcpServerConfig {
            env: BTreeMap::from([(
                "TOKEN".to_owned(),
                StdioEnvValue::FromChildEnv("NOPE".to_owned()),
            )]),
            ..config
        };
        assert!(missing.resolve_env(&child_env).is_err());
    }

    /// A tiny MCP server: answers tools/list with three tools and echoes
    /// every tools/call name back as its result.
    #[cfg(unix)]
    fn fake_server(dir: &std::path::Path) -> StdioServerLaunch {
        let script = dir.join("server.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"tools/list"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"get_issue"},{"name":"list_issues"},{"name":"delete_repo"}]}}\n' "$id" ;;
    *'"tools/call"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"called"}]}}\n' "$id" ;;
  esac
done
"#,
        )
        .unwrap();
        StdioServerLaunch {
            program: "/bin/sh".to_owned(),
            args: vec![script.to_string_lossy().into_owned()],
            env: HashMap::new(),
            env_removals: Vec::new(),
            cwd: dir.to_owned(),
            stderr_log: None,
            container_name_arg: None,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn relay_filters_a_live_session_and_stops_the_server() {
        let dir =
            std::env::temp_dir().join(format!("stashbase-mcp-stdio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let launch = fake_server(&dir);
        let policy = policy();
        let (agent, broker_side) = tokio::io::duplex(64 * 1024);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorded = events.clone();
        let server = tokio::spawn(async move {
            serve(broker_side, "github", &launch, &policy, |event, _| {
                recorded.lock().unwrap().push(match event {
                    RelayEvent::ToolCalled { tool } => format!("called:{tool}"),
                    RelayEvent::ToolRefused { tool, denial } => {
                        format!("refused:{}:{}", tool.unwrap_or("-"), denial.reason)
                    }
                });
            })
            .await
        });
        let (agent_read, mut agent_write) = tokio::io::split(agent);
        let mut agent_lines = BufReader::new(agent_read).lines();
        async fn send(writer: &mut (impl AsyncWrite + Unpin), line: &str) {
            writer
                .write_all(format!("{line}\n").as_bytes())
                .await
                .unwrap();
        }
        send(
            &mut agent_write,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        )
        .await;
        let listed: Value =
            serde_json::from_str(&agent_lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(listed["result"]["tools"][0]["name"], "get_issue");

        send(
            &mut agent_write,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"delete_repo"}}"#,
        )
        .await;
        let refused: Value =
            serde_json::from_str(&agent_lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(refused["id"], 2);
        assert_eq!(refused["error"]["code"], POLICY_DENIED_CODE);

        send(
            &mut agent_write,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_issue"}}"#,
        )
        .await;
        let called: Value =
            serde_json::from_str(&agent_lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(called["id"], 3);
        assert_eq!(called["result"]["content"][0]["text"], "called");

        // What `agent mcp connect` does when the agent closes its stdin.
        agent_write.shutdown().await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("the relay ends when the agent hangs up")
            .unwrap()
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["refused:delete_repo:tool_not_allowed", "called:get_issue"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Sends the upgrade request `agent mcp connect` sends and returns the
    /// status code and the stream, positioned after the response head.
    #[cfg(unix)]
    async fn open_broker(
        url: &str,
        name: &str,
        token: &str,
    ) -> (u16, BufReader<tokio::net::TcpStream>) {
        let url = reqwest::Url::parse(&format!("{url}/{name}")).unwrap();
        let address = format!("{}:{}", url.host_str().unwrap(), url.port().unwrap());
        let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
        stream
            .write_all(
                format!(
                    "GET {} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: upgrade\r\nUpgrade: {MCP_UPGRADE_PROTOCOL}\r\n\r\n",
                    url.path()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).await.unwrap();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).await.unwrap();
            if header == "\r\n" || header.is_empty() {
                break;
            }
        }
        (status, reader)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_proxy_serves_stdio_servers_only_to_the_run_token() {
        let dir =
            std::env::temp_dir().join(format!("stashbase-mcp-broker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let proxy =
            super::super::proxy::Proxy::start_with_port(HashMap::new(), policy(), None, None)
                .await
                .unwrap();
        let env =
            proxy.enable_mcp_broker(HashMap::from([("github".to_owned(), fake_server(&dir))]));
        let url = &env[MCP_BROKER_URL_ENV];
        let token = &env[MCP_BROKER_TOKEN_ENV];
        assert!(url.ends_with("/__stashbase/mcp"));

        assert_eq!(open_broker(url, "github", "wrong").await.0, 401);
        assert_eq!(open_broker(url, "linear", token).await.0, 404);

        let (status, reader) = open_broker(url, "github", token).await;
        assert_eq!(status, 101);
        let (read, mut write) = tokio::io::split(reader);
        let mut lines = BufReader::new(read).lines();
        write
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
            .await
            .unwrap();
        let listed: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1);
        write
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"delete_repo\"}}\n")
            .await
            .unwrap();
        let refused: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(refused["error"]["code"], POLICY_DENIED_CODE);
        write.shutdown().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(10), lines.next_line())
                .await
                .expect("the broker closes the connection once the agent hangs up")
                .unwrap()
                .is_none()
        );
        proxy.stop().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_server_that_exits_answers_pending_requests_with_its_log() {
        let launch = StdioServerLaunch {
            program: "/bin/sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                "read -r line; echo 'npm error Invalid Version' >&2".to_owned(),
            ],
            env: HashMap::new(),
            env_removals: Vec::new(),
            cwd: std::env::temp_dir(),
            stderr_log: None,
            container_name_arg: None,
        };
        let policy = policy();
        let (agent, broker_side) = tokio::io::duplex(64 * 1024);
        let relay =
            tokio::spawn(
                async move { serve(broker_side, "github", &launch, &policy, |_, _| {}).await },
            );
        let (read, mut write) = tokio::io::split(agent);
        write
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        let mut lines = BufReader::new(read).lines();
        let reply: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(10), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["error"]["code"], SERVER_EXITED_CODE);
        assert!(reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("MCP server 'github' exited before answering"));
        relay.await.unwrap().unwrap();
    }
}
