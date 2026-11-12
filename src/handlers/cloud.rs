//! `stashbase agent cloud setup|profile|create|list|run|ssh|stop|delete`: managed
//! cloud machines reached directly through the user's own `ssh`.

use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;

use crate::{
    api::cloud::{self, Machine},
    cmd::cloud::{CloudCommand, CloudSubcommand},
    handlers::run::entry::shell_quote,
    utils::{
        interaction::select,
        spinner::{new_spinner, Streams},
    },
};

pub async fn handle_cloud_command(
    cmd: CloudCommand,
    api_key: String,
    json: bool,
    silent: bool,
) -> Result<()> {
    match cmd.subcommand {
        CloudSubcommand::Setup(args) => setup(&api_key, args.rebuild, json, silent).await,
        CloudSubcommand::Profile(args) => profile(&api_key, &args.file, json, silent).await,
        CloudSubcommand::Create(_) => create(&api_key, json, silent).await,
        CloudSubcommand::List(_) => list(&api_key, json).await,
        CloudSubcommand::Run(args) => run(&api_key, args.id, args.command, json, silent).await,
        CloudSubcommand::Ssh(args) => connect(&api_key, &args.id, &[], json, silent).await,
        CloudSubcommand::Stop(args) => stop(&api_key, &args.id, json, silent).await,
        CloudSubcommand::Delete(args) => delete(&api_key, &args.id, json, silent).await,
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
        bail!("'{id}' is not a cloud machine id.")
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

/// Starts the one-time image preparation (or joins one in progress) and waits.
async fn setup(api_key: &str, rebuild: bool, json: bool, silent: bool) -> Result<()> {
    let mut status = cloud::setup_start(api_key, rebuild, json).await?;
    if !status.required {
        if !silent && !json {
            println!("No setup is needed for this deployment.");
        }
        return Ok(());
    }
    let mut spinner = (!silent && !json).then(|| {
        new_spinner(
            "Preparing the machine image (5-15 minutes)...",
            Streams::Stderr,
        )
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60 * 50);
    while status.state == "preparing" && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        status = cloud::setup_status(api_key, json).await?;
    }
    if let Some(mut spinner) = spinner.take() {
        spinner.clear();
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    }
    match status.state.as_str() {
        "ready" => {
            if !silent && !json {
                println!("The machine image is ready. Create a machine with: stashbase agent cloud create");
            }
            Ok(())
        }
        "error" => bail!(
            "Setup failed: {}. Run `stashbase agent cloud setup` to retry.",
            status
                .error
                .and_then(|e| e.message.or(e.code))
                .unwrap_or_else(|| "unknown error".into())
        ),
        _ => bail!("Setup is still running; run `stashbase agent cloud setup` to keep waiting."),
    }
}

async fn profile(api_key: &str, file: &Path, json: bool, silent: bool) -> Result<()> {
    let toml =
        fs::read_to_string(file).with_context(|| format!("Could not read {}.", file.display()))?;
    cloud::set_profile(api_key, &toml, json).await?;
    if !silent && !json {
        println!("Workspace machine profile updated.");
    }
    Ok(())
}

/// Creates a machine and keeps its key locally. The request takes a while,
/// so a spinner shows progress unless output is suppressed.
async fn create_machine(api_key: &str, json: bool, silent: bool) -> Result<Machine> {
    let keys = keys_dir()?;
    let pending = keys.join(format!("pending-{}", uuid::Uuid::new_v4()));
    let public_key = generate_key(&pending)?;
    let mut spinner =
        (!silent && !json).then(|| new_spinner("Creating the cloud machine...", Streams::Stderr));
    let known: Vec<String> = cloud::list(api_key, json)
        .await
        .map(|machines| machines.into_iter().map(|m| m.id).collect())
        .unwrap_or_default();
    let mut result = cloud::create(api_key, &public_key, json).await;
    if result.is_err() {
        // The server may drop the request while the VM is still being created:
        // look for the new machine instead of failing.
        for _ in 0..36 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let Ok(machines) = cloud::list(api_key, json).await else {
                continue;
            };
            if let Some(found) = machines.into_iter().find(|m| !known.contains(&m.id)) {
                match found.state.as_str() {
                    "running" => result = Ok(found),
                    "creating" => continue,
                    _ => {}
                }
                break;
            }
        }
    }
    if let Some(mut spinner) = spinner.take() {
        spinner.clear();
    }
    let machine = match result {
        Ok(machine) => machine,
        Err(error) => {
            let _ = fs::remove_dir_all(&pending);
            return Err(error);
        }
    };
    fs::rename(&pending, keys.join(&machine.id))?;
    Ok(machine)
}

async fn create(api_key: &str, json: bool, silent: bool) -> Result<()> {
    let machine = create_machine(api_key, json, silent).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&machine)?);
    } else if !silent {
        println!("Created cloud machine {}", machine.id);
        println!("Connect with: stashbase agent cloud run {}", machine.id);
    }
    Ok(())
}

async fn list(api_key: &str, json: bool) -> Result<()> {
    let machines = cloud::list(api_key, json).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&machines)?);
    } else if machines.is_empty() {
        println!("No cloud machines.");
    } else {
        for m in machines.iter().filter(|m| m.state != "deleted") {
            println!(
                "{}  {:<10}  {}",
                m.id,
                m.state,
                m.ip.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

/// Asks which machine to run on: one of the user's existing machines (with a
/// key on this computer), or a new one.
async fn pick_or_create(api_key: &str, json: bool, silent: bool) -> Result<String> {
    if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        bail!(
            "Pass a machine ID, or run in a terminal to pick a machine. \
             List machines with `stashbase agent cloud list`."
        );
    }
    let machines = cloud::list(api_key, json).await?;
    // A machine without a key from this CLI gets one added when you connect.
    let usable: Vec<Machine> = machines
        .into_iter()
        .filter(|m| matches!(m.state.as_str(), "running" | "stopped"))
        .collect();
    if usable.is_empty() {
        if !silent {
            eprintln!("No existing cloud machines; creating a new one.");
        }
        return Ok(create_machine(api_key, json, silent).await?.id);
    }
    let mut labels: Vec<String> = usable
        .iter()
        .map(|m| format!("{}  {:<8}", m.id, m.state))
        .collect();
    labels.push("Create a new machine".to_string());
    match select("Run on which cloud machine?", labels) {
        Some(index) if index < usable.len() => Ok(usable[index].id.clone()),
        Some(_) => Ok(create_machine(api_key, json, silent).await?.id),
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

/// Resumes the machine if it is stopped, then opens an interactive shell, or
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
    let machines = cloud::list(api_key, json).await?;
    let machine = machines
        .into_iter()
        .find(|m| m.id == id)
        .with_context(|| format!("No cloud machine {id}."))?;
    let machine = match machine.state.as_str() {
        "running" => machine,
        "stopped" | "error" => {
            let mut spinner =
                (!silent && !json).then(|| new_spinner("Starting the machine...", Streams::Stderr));
            let resumed = cloud::resume(api_key, id, json).await;
            if let Some(mut spinner) = spinner.take() {
                spinner.clear();
            }
            resumed?
        }
        other => bail!("Cloud machine {id} is {other}; try again shortly."),
    };
    let ip = machine.ip.context("The machine has no address yet.")?;
    // A machine launched elsewhere (for example from the dashboard without a key) has no key
    // from this CLI: make one and have the backend install it on the running machine.
    if !key.is_file() {
        let dir = keys_dir()?.join(id);
        let public_key = generate_key(&dir)?;
        if let Err(error) = cloud::authorize_key(api_key, id, &public_key, json).await {
            let _ = fs::remove_dir_all(&dir);
            return Err(error);
        }
        if !silent && !json {
            eprintln!("Added a key for this computer to the machine.");
        }
    }
    // The VM is new each time and addressed by IP, so host-key prompts add
    // nothing; retry the connection while sshd comes up.
    let mut ssh = Command::new("ssh");
    ssh.arg("-i")
        .arg(&key)
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "StrictHostKeyChecking=no"])
        .args(["-o", "UserKnownHostsFile=/dev/null"])
        .args(["-o", "LogLevel=ERROR"])
        .args(["-o", "ConnectTimeout=10"])
        .args(["-o", "ConnectionAttempts=30"]);
    if !command.is_empty() && std::io::stdin().is_terminal() {
        ssh.arg("-t");
    }
    ssh.arg(format!("agent@{ip}"));
    if !command.is_empty() {
        // ssh passes one string to the remote shell, so quote each argument.
        let remote = command
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        ssh.arg(remote);
    } else if std::io::stdin().is_terminal() {
        // An interactive shell attaches to the machine's shared tmux session, the same one the
        // dashboard terminal uses, so both show the same running programs. Without tmux, a
        // plain login shell.
        ssh.arg("-t").arg(
            "sh -c 'infocmp \"$TERM\" >/dev/null 2>&1 || TERM=xterm-256color; export TERM; if command -v tmux >/dev/null 2>&1; then exec tmux new-session -A -s main; else exec \"$SHELL\" -l; fi'",
        );
    }
    let status = ssh
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
        println!("Stopped cloud machine {id}");
        println!("Start it again with: stashbase agent cloud run {id}");
    }
    Ok(())
}

async fn delete(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    cloud::destroy(api_key, id, json).await?;
    let _ = fs::remove_dir_all(keys_dir()?.join(id));
    if !silent && !json {
        println!("Deleted cloud machine {id}");
    }
    Ok(())
}
