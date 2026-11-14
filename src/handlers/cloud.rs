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
        CloudSubcommand::Run(args) => {
            run(&api_key, args.id, args.command, !args.no_tmux, json, silent).await
        }
        CloudSubcommand::Ssh(args) => {
            connect(&api_key, &args.id, &[], !args.no_tmux, json, silent).await
        }
        CloudSubcommand::Stop(args) => stop(&api_key, &args.id, json, silent).await,
        CloudSubcommand::Delete(args) => delete(&api_key, &args.id, json, silent).await,
    }
}

fn keys_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "stashbase").context("Could not find data directory.")?;
    Ok(dirs.data_local_dir().join("cloud"))
}

fn key_path(id: &str) -> Result<PathBuf> {
    Ok(keys_dir()?
        .join(machine_uuid(id)?.to_string())
        .join("id_ed25519"))
}

/// Match the backend's short-uuid default (Flickr base58), keeping local keys under
/// the full UUID so existing machines reuse their keys after the API format changes.
fn machine_uuid(id: &str) -> Result<uuid::Uuid> {
    if id.len() == 36 {
        if let Ok(value) = uuid::Uuid::parse_str(id) {
            return Ok(value);
        }
    }
    if let Some(short) = id.strip_prefix("vm_").filter(|s| s.len() == 22) {
        const ALPHABET: &[u8] = b"123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
        let value = short.bytes().try_fold(0_u128, |value, character| {
            let digit = ALPHABET.iter().position(|&c| c == character)? as u128;
            value.checked_mul(58)?.checked_add(digit)
        });
        if let Some(value) = value {
            return Ok(uuid::Uuid::from_u128(value));
        }
    }
    bail!("'{id}' is not a cloud machine id. Use a vm_ ID from `stashbase agent cloud list`.")
}

fn validate_id(id: &str) -> Result<()> {
    machine_uuid(id).map(|_| ())
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

/// Asks for a certificate, waiting while a new machine is still starting up and has not
/// reported its host key yet (about two minutes at most).
async fn certificate_when_ready(
    api_key: &str,
    id: &str,
    public_key: &str,
    json: bool,
    silent: bool,
) -> Result<cloud::Certificate> {
    let mut spinner = None;
    let mut attempt = 0;
    loop {
        match cloud::certificate(api_key, id, public_key, json).await {
            Err(error) if attempt < 24 && format!("{error:#}").contains("host_key_unavailable") => {
                if spinner.is_none() && !silent && !json {
                    spinner = Some(new_spinner(
                        "Waiting for the machine to finish starting...",
                        Streams::Stderr,
                    ));
                }
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            other => {
                if let Some(mut spinner) = spinner.take() {
                    spinner.clear();
                }
                return other;
            }
        }
    }
}

/// The `<type> <base64>` public key of the key pair in `dir`.
fn read_public_key(dir: &Path) -> Result<String> {
    let public = fs::read_to_string(dir.join("id_ed25519.pub"))
        .context("Could not read this computer's key for the machine.")?;
    let mut parts = public.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(kind), Some(data)) => Ok(format!("{kind} {data}")),
        _ => bail!("Could not read this computer's key for the machine."),
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
    fs::rename(&pending, keys.join(machine_uuid(&machine.id)?.to_string()))?;
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
    use_tmux: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    let id = match id {
        Some(id) => id,
        None => pick_or_create(api_key, json, silent).await?,
    };
    connect(api_key, &id, &command, use_tmux, json, silent).await
}

/// Resumes the machine if it is stopped, then opens an interactive shell, or
/// runs `command` there, through the user's own `ssh`.
async fn connect(
    api_key: &str,
    id: &str,
    command: &[String],
    use_tmux: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    validate_id(id)?;
    let key = key_path(id)?;
    let machines = cloud::list(api_key, json).await?;
    let machine = machines
        .into_iter()
        .find(|m| machine_uuid(&m.id).ok() == machine_uuid(id).ok())
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
    // Every connection uses a short-lived certificate for this computer's key, issued by the
    // backend for this machine only, and checks the machine's host key against the one it
    // reported at boot. A machine launched elsewhere (for example from the dashboard without a
    // key) gets a key made here first.
    let dir = key
        .parent()
        .context("Missing cloud key directory.")?
        .to_path_buf();
    let had_key = key.is_file();
    if !had_key {
        generate_key(&dir)?;
    }
    let public_key = read_public_key(&dir)?;
    let cert_path = dir.join("id_ed25519-cert.pub");
    let known_hosts = dir.join("known_hosts");
    let mut target = ip.clone();
    let pinned = match certificate_when_ready(api_key, id, &public_key, json, silent).await {
        Ok(issued) => {
            fs::write(&cert_path, format!("{}\n", issued.certificate.trim()))?;
            fs::write(
                &known_hosts,
                format!("{} {}\n", issued.ip, issued.host_key.trim()),
            )?;
            target = issued.ip;
            true
        }
        Err(error) => {
            let message = format!("{error:#}");
            if message.contains("host_key_unavailable") {
                if !had_key {
                    let _ = fs::remove_dir_all(&dir);
                }
                bail!("The machine has not reported its host key yet. It is probably still starting; try again in a minute.");
            }
            // A machine created before certificate access existed can only be reached with its
            // own key, and its host key cannot be verified.
            if had_key && message.contains("terminal_unavailable") {
                if !silent && !json {
                    eprintln!(
                        "Warning: this machine predates certificate access; its host key is not verified."
                    );
                }
                false
            } else {
                if !had_key {
                    let _ = fs::remove_dir_all(&dir);
                }
                return Err(error);
            }
        }
    };
    let mut ssh = Command::new("ssh");
    ssh.arg("-i")
        .arg(&key)
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "LogLevel=ERROR"])
        .args(["-o", "ConnectTimeout=10"])
        .args(["-o", "ConnectionAttempts=30"]);
    if pinned {
        ssh.arg("-o")
            .arg(format!("CertificateFile={}", cert_path.display()))
            .args(["-o", "StrictHostKeyChecking=yes"])
            .args(["-o", "GlobalKnownHostsFile=/dev/null"])
            .args(["-o", "UpdateHostKeys=no"])
            .arg("-o")
            .arg(format!("UserKnownHostsFile={}", known_hosts.display()));
    } else {
        ssh.args(["-o", "StrictHostKeyChecking=no"])
            .args(["-o", "UserKnownHostsFile=/dev/null"]);
    }
    if !command.is_empty() && std::io::stdin().is_terminal() {
        ssh.arg("-t");
    }
    ssh.arg(format!("agent@{target}"));
    if !command.is_empty() {
        // ssh passes one string to the remote shell, so quote each argument.
        let remote = command
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        ssh.arg(remote);
    } else if std::io::stdin().is_terminal() {
        // An interactive shell attaches to the machine's shared tmux session by default, the
        // same one the dashboard terminal uses, so both show the same running programs. Without
        // tmux, or with --no-tmux, a plain login shell. Either way an unknown local terminal
        // type falls back to xterm-256color.
        let shell = if use_tmux {
            "if command -v tmux >/dev/null 2>&1; then exec tmux new-session -A -s main; else exec \"$SHELL\" -l; fi"
        } else {
            "exec \"$SHELL\" -l"
        };
        ssh.arg("-t").arg(format!(
            "sh -c 'infocmp \"$TERM\" >/dev/null 2>&1 || TERM=xterm-256color; export TERM; {shell}'"
        ));
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
