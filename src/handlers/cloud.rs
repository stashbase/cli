//! `stashbase agent cloud create|list|run|ssh|stop|delete`: managed cloud
//! machines reached through the user's own `ssh`, tunnelled over the
//! Stashbase API.

use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::{
    api::{
        client,
        cloud::{self, CloudSession},
    },
    cmd::cloud::{CloudCommand, CloudSubcommand},
    handlers::run::entry::shell_quote,
    utils::{
        interaction::select,
        spinner::{new_spinner, Streams},
    },
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
        CloudSubcommand::Run(args) => run(&api_key, args.id, args.command, json, silent).await,
        CloudSubcommand::Ssh(args) => connect(&api_key, &args.id, &[], json, silent).await,
        CloudSubcommand::Stop(args) => stop(&api_key, &args.id, json, silent).await,
        CloudSubcommand::Delete(args) => delete(&api_key, &args.id, json, silent).await,
        CloudSubcommand::Tunnel(args) => tunnel(&args.id).await,
    }
}

fn keys_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "stashbase").context("Could not find data directory.")?;
    Ok(dirs.data_local_dir().join("cloud"))
}

fn key_path(id: &str) -> Result<PathBuf> {
    Ok(keys_dir()?.join(id).join("id_ed25519"))
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

/// Creates a machine and keeps its key locally. The request takes a while,
/// so a spinner shows progress unless output is suppressed.
async fn create_session(api_key: &str, json: bool, silent: bool) -> Result<CloudSession> {
    let keys = keys_dir()?;
    let pending = keys.join(format!("pending-{}", uuid::Uuid::new_v4()));
    let public_key = generate_key(&pending)?;
    let mut spinner =
        (!silent && !json).then(|| new_spinner("Creating the cloud machine...", Streams::Stderr));
    let result = cloud::create(api_key, &public_key, json).await;
    if let Some(mut spinner) = spinner.take() {
        spinner.clear();
    }
    let session = match result {
        Ok(session) => session,
        Err(error) => {
            let _ = fs::remove_dir_all(&pending);
            return Err(error);
        }
    };
    fs::rename(&pending, keys.join(&session.id))?;
    Ok(session)
}

async fn create(api_key: &str, json: bool, silent: bool) -> Result<()> {
    let session = create_session(api_key, json, silent).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&session)?);
    } else if !silent {
        println!("Created cloud session {}", session.id);
        println!("Connect with: stashbase agent cloud run {}", session.id);
    }
    Ok(())
}

async fn list(api_key: &str, json: bool) -> Result<()> {
    let sessions = cloud::list(api_key, json).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else if sessions.is_empty() {
        println!("No cloud sessions.");
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

fn picker_label(session: &CloudSession) -> String {
    format!(
        "{}  {:<8}  expires {}",
        session.id, session.status, session.expires_at
    )
}

/// Asks which machine to run on: one of the user's existing machines (ready
/// or stopped, with a key on this computer), or a new one.
async fn pick_or_create(api_key: &str, json: bool, silent: bool) -> Result<String> {
    if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        bail!(
            "Pass a session ID, or run in a terminal to pick a machine. \
             List machines with `stashbase agent cloud list`."
        );
    }
    let sessions = cloud::list(api_key, json).await?;
    // A machine created on another computer has no key here, so it cannot be used.
    let usable: Vec<CloudSession> = sessions
        .into_iter()
        .filter(|s| matches!(s.status.as_str(), "ready" | "stopped"))
        .filter(|s| key_path(&s.id).map(|path| path.is_file()).unwrap_or(false))
        .collect();
    if usable.is_empty() {
        if !silent {
            eprintln!("No existing cloud machines; creating a new one.");
        }
        return Ok(create_session(api_key, json, silent).await?.id);
    }
    let mut labels: Vec<String> = usable.iter().map(picker_label).collect();
    labels.push("Create a new machine".to_string());
    match select("Run on which cloud machine?", labels) {
        Some(index) if index < usable.len() => Ok(usable[index].id.clone()),
        Some(_) => Ok(create_session(api_key, json, silent).await?.id),
        None => bail!("Cancelled."),
    }
}

async fn run(
    api_key: &str,
    id: Option<String>,
    command: Vec<String>,
    json: bool,
    silent: bool,
) -> Result<()> {
    let id = match id {
        Some(id) => id,
        None => pick_or_create(api_key, json, silent).await?,
    };
    connect(api_key, &id, &command, json, silent).await
}

/// Starts the machine if it is stopped, then opens an interactive shell, or
/// runs `command` there, through the user's own `ssh`.
async fn connect(
    api_key: &str,
    id: &str,
    command: &[String],
    json: bool,
    silent: bool,
) -> Result<()> {
    validate_id(id)?;
    let key = key_path(id)?;
    if !key.is_file() {
        bail!("No local key for cloud session {id}; it was created on another machine.");
    }
    // Attaching starts a stopped machine, which can take a while.
    let mut spinner =
        (!silent && !json).then(|| new_spinner("Starting the machine...", Streams::Stderr));
    let attach = cloud::attach(api_key, id, json).await;
    if let Some(mut spinner) = spinner.take() {
        spinner.clear();
    }
    let attach = attach?;
    let exe = std::env::current_exe().context("Could not locate the stashbase binary.")?;
    let proxy = format!(
        "{} agent cloud tunnel {id}",
        shell_quote(&exe.to_string_lossy())
    );
    // The key is a throwaway for this session and the host is reached through
    // an authenticated tunnel, so host-key prompts add nothing here.
    let mut ssh = Command::new("ssh");
    ssh.arg("-i")
        .arg(&key)
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .args(["-o", "LogLevel=ERROR"])
        .arg("-o")
        .arg(format!("ProxyCommand={proxy}"));
    if !command.is_empty() && std::io::stdin().is_terminal() {
        // A command in a terminal gets one, so interactive programs work.
        ssh.arg("-t");
    }
    ssh.arg(format!("{}@cloud-{id}", attach.ssh_user));
    if !command.is_empty() {
        // ssh passes one string to the remote shell, so quote each argument.
        let remote = command
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        ssh.arg(remote);
    }
    let status = ssh
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
    // The key stays: `run` or `ssh` starts the machine again.
    if !silent && !json {
        println!("Stopped cloud session {id}");
        println!("Start it again with: stashbase agent cloud run {id}");
    }
    Ok(())
}

async fn delete(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    cloud::destroy(api_key, id, json).await?;
    let _ = fs::remove_dir_all(keys_dir()?.join(id));
    if !silent && !json {
        println!("Deleted cloud session {id}");
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
