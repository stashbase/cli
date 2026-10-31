//! `stashbase agent mcp connect <server>`: the stdio MCP server an agent is
//! configured to start. It holds no policy. It opens an upgraded connection
//! to the run's Agent Proxy, which starts the real server and checks every
//! message against the profile, and copies bytes between that connection and
//! its own stdin and stdout.

use anyhow::{bail, Context, Result};
use http_body_util::{BodyExt, Empty};
use hyper::{body::Bytes, header, Request, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::{io::AsyncWriteExt, net::TcpStream};

use crate::{
    cmd::agent::AgentMcpConnectCommand,
    handlers::{
        agent::sessions::{mcp_broker_for_ancestors, McpBrokerEndpoint},
        run::mcp_stdio::{MCP_BROKER_TOKEN_ENV, MCP_BROKER_URL_ENV, MCP_UPGRADE_PROTOCOL},
    },
};

pub async fn handle_agent_mcp_connect_command(command: AgentMcpConnectCommand) -> Result<()> {
    let endpoint = broker_endpoint(
        std::env::var(MCP_BROKER_URL_ENV).ok(),
        std::env::var(MCP_BROKER_TOKEN_ENV).ok(),
        mcp_broker_for_ancestors,
    )
    .map_err(|reason| {
        anyhow::anyhow!(
            "agent mcp connect only works inside 'stashbase agent run' with a profile that declares [mcp_servers.{}] command ({reason})",
            command.server
        )
    })?;
    let url = reqwest::Url::parse(&format!("{}/{}", endpoint.url, command.server))
        .context("the run's MCP broker address is invalid")?;
    let host = url
        .host_str()
        .context("the run's MCP broker address has no host")?;
    let port = url
        .port_or_known_default()
        .context("the run's MCP broker address has no port")?;
    let stream = TcpStream::connect((host, port))
        .await
        .context("cannot reach the run's Agent Proxy; has the run ended?")?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(connection.with_upgrades());
    let request = Request::get(url.path())
        .header(header::HOST, format!("{host}:{port}"))
        .header(header::AUTHORIZATION, format!("Bearer {}", endpoint.token))
        .header(header::CONNECTION, "upgrade")
        .header(header::UPGRADE, MCP_UPGRADE_PROTOCOL)
        .body(Empty::<Bytes>::new())?;
    let response = sender.send_request(request).await?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        let status = response.status();
        let body = response.into_body().collect().await?.to_bytes();
        bail!("{}", broker_refusal(status, &body));
    }
    let upgraded = hyper::upgrade::on(response).await?;
    let (mut from_broker, mut to_broker) = tokio::io::split(TokioIo::new(upgraded));
    let to_server = async {
        let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut to_broker).await;
        // The agent closed our stdin: tell the broker, which lets the server
        // answer what it already has and then closes the connection.
        let _ = to_broker.shutdown().await;
        std::future::pending::<()>().await
    };
    let to_agent = async {
        let mut stdout = tokio::io::stdout();
        let result = tokio::io::copy(&mut from_broker, &mut stdout).await;
        let _ = stdout.flush().await;
        result
    };
    // Done when the broker side ends, whether because the agent hung up or
    // the server exited, not when stdin does.
    tokio::select! {
        result = to_agent => { result?; }
        () = to_server => {}
    }
    Ok(())
}

/// The run's broker: from the environment `agent run` gives the agent, or,
/// for agents that start MCP servers with a cleared environment (Codex),
/// from the session file of the run this process descends from.
fn broker_endpoint(
    url: Option<String>,
    token: Option<String>,
    from_ancestors: impl FnOnce() -> std::result::Result<McpBrokerEndpoint, String>,
) -> std::result::Result<McpBrokerEndpoint, String> {
    match (url, token) {
        (Some(url), Some(token)) if !url.is_empty() && !token.is_empty() => {
            Ok(McpBrokerEndpoint { url, token })
        }
        _ => from_ancestors(),
    }
}

/// The proxy's error message, which names what is wrong (unknown server,
/// stale token), or the status alone.
fn broker_refusal(status: StatusCode, body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format!("the run's MCP broker refused the connection ({status})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(url: &str) -> McpBrokerEndpoint {
        McpBrokerEndpoint {
            url: url.to_owned(),
            token: "t".to_owned(),
        }
    }

    #[test]
    fn the_environment_wins_over_the_session_file() {
        let found = broker_endpoint(
            Some("http://127.0.0.1:1/__stashbase/mcp".to_owned()),
            Some("t".to_owned()),
            || panic!("the session file is only read without the environment"),
        );
        assert_eq!(found, Ok(endpoint("http://127.0.0.1:1/__stashbase/mcp")));
    }

    #[test]
    fn a_cleared_environment_falls_back_to_the_session_file() {
        // Codex starts MCP servers with only HOME, PATH and the like.
        let found = broker_endpoint(None, None, || {
            Ok(endpoint("http://127.0.0.1:2/__stashbase/mcp"))
        });
        assert_eq!(found, Ok(endpoint("http://127.0.0.1:2/__stashbase/mcp")));
        let partial = broker_endpoint(Some("http://x".to_owned()), None, || Err("none".to_owned()));
        assert_eq!(partial, Err("none".to_owned()));
    }

    #[test]
    fn refusals_carry_the_proxy_message() {
        let body = br#"{"error":{"code":"proxy.mcp_server_not_found","message":"No stdio MCP server named 'x' in this run's profile"}}"#;
        assert_eq!(
            broker_refusal(StatusCode::NOT_FOUND, body),
            "No stdio MCP server named 'x' in this run's profile"
        );
        assert!(broker_refusal(StatusCode::UNAUTHORIZED, b"").contains("401"));
    }
}
