//! `stashbase cloud create|list|ssh|stop`: managed cloud sessions reached
//! through the user's own `ssh`, tunnelled over the Stashbase API.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::{
    api::{client, cloud},
    cmd::cloud::{CloudCommand, CloudSubcommand},
    handlers::run::entry::shell_quote,
};

const TUNNEL_TOKEN_ENV: &str = "STASHBASE_CLOUD_TUNNEL_TOKEN";

pub async fn handle_cloud_command(
    cmd: CloudCommand,
    api_key: String,
    json: bool,
    silent: bool,
) -> Result<()> {
    match cmd.subcommand {
        CloudSubcommand::Create(_) => create(&api_key, json, silent).await,
        CloudSubcommand::List(_) => list(&api_key, json).await,
        CloudSubcommand::Ssh(args) => ssh(&api_key, &args.id, json).await,
        CloudSubcommand::Stop(args) => stop(&api_key, &args.id, json, silent).await,
        CloudSubcommand::Tunnel(args) => tunnel(&args.id).await,
    }
}

fn keys_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "stashbase").context("Could not find data directory.")?;
    Ok(dirs.data_local_dir().join("cloud"))
}

fn validate_id(id: &str) -> Result<()> {
    if id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        Ok(())
    } else {
        bail!("'{id}' is not a cloud session id.")
    }
}

/// Generates a throwaway ed25519 key with the user's own `ssh-keygen`.
fn generate_key(dir: &Path) -> Result<String> {
    fs::create_dir_all(dir)?;
    let key = dir.join("id_ed25519");
    let status = Command::new("ssh-keygen")
        .args([
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-C",
            "stashbase-cloud",
            "-f",
        ])
        .arg(&key)
        .status()
        .context("`ssh-keygen` was not found on this machine.")?;
    if !status.success() {
        bail!("ssh-keygen failed.");
    }
    let public = fs::read_to_string(dir.join("id_ed25519.pub"))?;
    // Send only "<type> <base64>"; the server rejects options and comments.
    let mut parts = public.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(kind), Some(data)) => Ok(format!("{kind} {data}")),
        _ => bail!("Could not read the generated public key."),
    }
}

async fn create(api_key: &str, json: bool, silent: bool) -> Result<()> {
    let keys = keys_dir()?;
    let pending = keys.join(format!("pending-{}", uuid::Uuid::new_v4()));
    let public_key = generate_key(&pending)?;
    let session = match cloud::create(api_key, &public_key, json).await {
        Ok(session) => session,
        Err(error) => {
            let _ = fs::remove_dir_all(&pending);
            return Err(error);
        }
    };
    fs::rename(&pending, keys.join(&session.id))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&session)?);
    } else if !silent {
        println!("Created cloud session {}", session.id);
        println!("Connect with: stashbase agent cloud ssh {}", session.id);
    }
    Ok(())
}

async fn list(api_key: &str, json: bool) -> Result<()> {
    let sessions = cloud::list(api_key, json).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else if sessions.is_empty() {
        println!("No active cloud sessions.");
    } else {
        for session in sessions {
            println!(
                "{}  {:<12}  expires {}",
                session.id, session.status, session.expires_at
            );
        }
    }
    Ok(())
}

async fn ssh(api_key: &str, id: &str, json: bool) -> Result<()> {
    validate_id(id)?;
    let key = keys_dir()?.join(id).join("id_ed25519");
    if !key.is_file() {
        bail!("No local key for cloud session {id}; it was created on another machine.");
    }
    let attach = cloud::attach(api_key, id, json).await?;
    let exe = std::env::current_exe().context("Could not locate the stashbase binary.")?;
    let proxy = format!(
        "{} agent cloud tunnel {id}",
        shell_quote(&exe.to_string_lossy())
    );
    // The key is a throwaway for this session and the host is reached through
    // an authenticated tunnel, so host-key prompts add nothing here.
    let status = Command::new("ssh")
        .arg("-i")
        .arg(&key)
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .args(["-o", "LogLevel=ERROR"])
        .arg("-o")
        .arg(format!("ProxyCommand={proxy}"))
        .arg(format!("{}@cloud-{id}", attach.ssh_user))
        .env(TUNNEL_TOKEN_ENV, &attach.tunnel_token)
        .status()
        .context("`ssh` was not found on this machine.")?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

async fn stop(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    cloud::stop(api_key, id, json).await?;
    let _ = fs::remove_dir_all(keys_dir()?.join(id));
    if !silent && !json {
        println!("Stopped cloud session {id}");
    }
    Ok(())
}

/// Bridges stdin/stdout to the API tunnel WebSocket so `ssh` can use it as a
/// `ProxyCommand`.
async fn tunnel(id: &str) -> Result<()> {
    validate_id(id)?;
    let token = std::env::var(TUNNEL_TOKEN_ENV).context("Missing tunnel token.")?;
    let base = client::get_api_url()
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let mut request = format!("{base}/v1/cloud/sessions/{id}/tunnel").into_client_request()?;
    request
        .headers_mut()
        .insert("x-stashbase-tunnel-token", token.parse()?);
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("Could not open the tunnel to the cloud session.")?;
    let (mut write, mut read) = socket.split();

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            count = stdin.read(&mut buffer) => {
                let count = count?;
                if count == 0 {
                    let _ = write.send(Message::Close(None)).await;
                    break;
                }
                write.send(Message::Binary(buffer[..count].to_vec().into())).await?;
            }
            message = read.next() => match message {
                Some(Ok(Message::Binary(data))) => {
                    stdout.write_all(&data).await?;
                    stdout.flush().await?;
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
            }
        }
    }
    Ok(())
}
