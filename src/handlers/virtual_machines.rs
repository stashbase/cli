//! `stashbase vms create|list|info|metrics|rename|autosuspend|usage|connect|start|stop|delete`: managed
//! virtual machines reached directly through the user's own `ssh`.

use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use tabled::Tabled;
use tokio::time::{sleep, timeout};

use crate::{
    api::virtual_machines::{self, Machine},
    cmd::virtual_machines::{
        PortForward, VirtualMachineCommand, VirtualMachineConnect, VirtualMachineSubcommand,
    },
    handlers::run::entry::shell_quote,
    utils::{
        interaction::{confirm_text, input_optional, select},
        output::get_formatted_json_string,
        spinner::{new_spinner, request_spinner, Streams},
        tables::build::build_table,
    },
};

pub async fn handle_virtual_machine_command(
    cmd: VirtualMachineCommand,
    api_key: String,
    json: bool,
    silent: bool,
) -> Result<()> {
    // Errors print after an empty line, like the other commands; only the message is shown, not
    // the network error kept underneath it.
    run_command(cmd, api_key, json, silent)
        .await
        .map_err(|error| {
            if json {
                error
            } else {
                anyhow::anyhow!("\n{error}")
            }
        })
}

async fn run_command(
    cmd: VirtualMachineCommand,
    api_key: String,
    json: bool,
    silent: bool,
) -> Result<()> {
    match cmd.subcommand {
        VirtualMachineSubcommand::Create(args) => {
            create(
                &api_key,
                args.location.as_deref(),
                args.name.as_deref(),
                args.autosuspend == "on",
                json,
                silent,
            )
            .await
        }
        VirtualMachineSubcommand::List(_) => list(&api_key, json, silent).await,
        VirtualMachineSubcommand::Info(args) => info(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Metrics(args) => metrics(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Rename(args) => {
            rename(&api_key, &args.id, &args.name, json, silent).await
        }
        VirtualMachineSubcommand::Autosuspend(args) => {
            autosuspend(&api_key, &args.id, args.mode == "on", json, silent).await
        }
        VirtualMachineSubcommand::Usage(_) => usage(&api_key, json, silent).await,
        VirtualMachineSubcommand::Connect(args) => run(&api_key, args, json, silent).await,
        VirtualMachineSubcommand::Start(args) => start(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Stop(args) => stop(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Cp(args) => {
            cp(
                &api_key,
                &args.source,
                &args.destination,
                args.recursive,
                json,
                silent,
            )
            .await
        }
        VirtualMachineSubcommand::Delete(args) => {
            delete(&api_key, &args.id, args.yes, json, silent).await
        }
    }
}

/// A path as an `ssh -o` value. `ssh` reads the option like a config line, so a path with a space
/// (for example `~/Library/Application Support/...` on macOS) must be quoted.
fn ssh_option_path(path: &Path) -> Result<String> {
    let text = path.display().to_string();
    if text.contains('"') || text.contains('\n') {
        bail!("The path {text} cannot be passed to ssh");
    }
    Ok(format!("\"{text}\""))
}

fn keys_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "stashbase").context("Could not find data directory.")?;
    Ok(dirs.data_local_dir().join("vms"))
}

/// Removes the key folders of machines that are no longer listed (deleted from the dashboard, for
/// example; `vms delete` removes its own). Only folders untouched for a day go: they may belong to
/// a machine in another workspace, and a removed folder costs nothing more than a new key and
/// certificate on the next connect. Best effort; errors are ignored.
fn prune_key_dirs(machines: &[Machine]) {
    let Ok(keys) = keys_dir() else { return };
    let Ok(entries) = fs::read_dir(&keys) else {
        return;
    };
    let listed: std::collections::HashSet<uuid::Uuid> = machines
        .iter()
        .filter_map(|m| machine_uuid(&m.id).ok())
        .collect();
    let day = Duration::from_secs(24 * 60 * 60);
    let recent = |path: &std::path::Path| {
        let newest = fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.metadata().ok()?.modified().ok())
            .chain(fs::metadata(path).ok().and_then(|m| m.modified().ok()))
            .max();
        newest.is_none_or(|time| time.elapsed().map_or(true, |age| age < day))
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // A machine's folder is its UUID; `pending-…` is a create that did not finish.
        let stale = match uuid::Uuid::parse_str(&name) {
            Ok(id) => !listed.contains(&id),
            Err(_) => name.starts_with("pending-"),
        };
        if stale && path.is_dir() && !recent(&path) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

fn machine_key_dir(id: &str) -> Result<PathBuf> {
    Ok(keys_dir()?.join(machine_uuid(id)?.to_string()))
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
    bail!("'{id}' is not a virtual machine id. Use a vm_ ID from `stashbase vms list`.")
}

fn validate_id(id: &str) -> Result<()> {
    machine_uuid(id).map(|_| ())
}

/// This computer's key for a machine, made with the user's own `ssh-keygen` on first use.
/// Concurrent first connections agree on one key: each generates into its own folder and the
/// first to link its key into place wins; the public key is always read from the key in place.
fn load_or_create_key(dir: &Path) -> Result<String> {
    let key = dir.join("id_ed25519");
    if !key.is_file() {
        let staging = dir.join(format!("keygen-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging)?;
        let staged = staging.join("id_ed25519");
        let generated = Command::new("ssh-keygen")
            .args([
                "-q",
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                "stashbase-virtual-machine",
                "-f",
            ])
            .arg(&staged)
            .stdin(std::process::Stdio::null())
            .status()
            .context("`ssh-keygen` was not found on this machine.")
            .and_then(|status| {
                if !status.success() {
                    bail!("ssh-keygen failed.");
                }
                match fs::hard_link(&staged, &key) {
                    Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                        Err(error).context("Could not save this computer's key for the machine.")
                    }
                    _ => Ok(()),
                }
            });
        let _ = fs::remove_dir_all(&staging);
        generated?;
    }
    let output = Command::new("ssh-keygen")
        .args(["-y", "-f"])
        .arg(&key)
        .stdin(std::process::Stdio::null())
        .output()
        .context("`ssh-keygen` was not found on this machine.")?;
    let public = String::from_utf8_lossy(&output.stdout);
    // Send only "<type> <base64>"; the server rejects options and comments.
    let mut parts = public.split_whitespace();
    match (output.status.success(), parts.next(), parts.next()) {
        (true, Some(kind), Some(data)) => Ok(format!("{kind} {data}")),
        _ => bail!("Could not read this computer's key for the machine."),
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
) -> Result<virtual_machines::Certificate> {
    let mut spinner = None;
    let result = timeout(Duration::from_secs(120), async {
        loop {
            match virtual_machines::certificate(api_key, id, public_key, json).await {
                Err(error) if format!("{error:#}").contains("host_key_unavailable") => {
                    if spinner.is_none() && !silent && !json {
                        spinner = Some(new_spinner(
                            "Waiting for the machine to finish booting...",
                            Streams::Stderr,
                        ));
                    }
                    virtual_machines::pause(Duration::from_secs(5), json).await?;
                }
                result => return result,
            }
        }
    })
    .await;
    if let Some(mut spinner) = spinner {
        spinner.clear();
    }
    result.with_context(|| {
        format!("Timed out waiting for an SSH certificate for virtual machine {id}. Try connecting again shortly.")
    })?
}

/// Creates a machine reached only with certificates (no login key). The server answers once the machine is recorded
/// and keeps creating it in the cloud (about a minute); with `wait`, this follows it until it is
/// running (`connect` needs that), otherwise it returns the machine while it is being created.
async fn create_machine(
    api_key: &str,
    location: &str,
    name: Option<&str>,
    wait: bool,
    json: bool,
    silent: bool,
) -> Result<Machine> {
    // The machine gets no login key: it is reached only with short-lived certificates, so no
    // `authorized_keys` entry outlives a revoked API key or a disabled 2FA. The local key is made
    // on the first connect, so creating needs no local SSH tools.
    let mut spinner =
        (!silent && !json).then(|| new_spinner("Creating the virtual machine...", Streams::Stderr));
    let mut result = virtual_machines::create(api_key, location, name, json)
        .await
        .map_err(|error| {
            // A lost answer or a Ctrl+C mid-request: the server may have recorded the machine
            // anyway, and a blind retry could make a second one. On a Ctrl+C the CLI prints only
            // "Request aborted", so the warning is printed here. `--json` keeps its one document.
            const UNCONFIRMED: &str = "Could not confirm whether the virtual machine was created. Check `stashbase vms list` before retrying.";
            if json {
                error
            } else if virtual_machines::aborted() {
                eprintln!("{UNCONFIRMED}");
                error
            } else if error.downcast_ref::<virtual_machines::NoAnswer>().is_some() {
                error.context(UNCONFIRMED)
            } else {
                error
            }
        });
    if wait {
        if let Ok(created) = &result {
            if created.state == "creating"
                || (created.state == "running" && created.ready == Some(false))
            {
                let id = created.id.clone();
                result = timeout(Duration::from_secs(240), async {
                    loop {
                        virtual_machines::pause(Duration::from_secs(3), json).await?;
                        let Ok(machines) = virtual_machines::list(api_key, json).await else {
                            continue;
                        };
                        match machines.into_iter().find(|m| m.id == id) {
                            Some(m) if m.state == "creating" => continue,
                            Some(m) if m.state == "running" && m.ready != Some(false) => {
                                return Ok(m);
                            }
                            Some(m) if m.state == "running" => continue,
                            Some(m) => bail!(
                                "The machine could not be created ({}). Delete it with `stashbase vms delete {id}` and try again.",
                                m.error.unwrap_or(m.state)
                            ),
                            None => continue,
                        }
                    }
                })
                .await
                .with_context(|| {
                    format!("Timed out waiting for virtual machine {id} to become ready. Check its status with `stashbase vms info {id}`.")
                })
                .and_then(|result| result);
            }
        }
    }
    if let Some(mut spinner) = spinner.take() {
        spinner.clear();
    }
    result
}

async fn create(
    api_key: &str,
    location: Option<&str>,
    name: Option<&str>,
    autosuspend: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if location.is_none() && !interactive {
        bail!("Pass --location us, eu or asia.");
    }
    let location = region(api_key, location, json).await?;
    // The name is optional: asked for in a terminal (Enter skips it), never required in scripts.
    let asked;
    let name = match name {
        Some(name) => Some(name.trim()).filter(|n| !n.is_empty()),
        None if interactive => {
            asked = input_optional("Name (optional, press Enter to skip)")
                .ok_or_else(|| anyhow::anyhow!("Cancelled."))?;
            Some(asked.trim()).filter(|n| !n.is_empty())
        }
        None => None,
    };
    let mut machine = create_machine(api_key, &location, name, false, json, silent).await?;
    // New machines suspend when idle; turning it off is a second call once the machine exists.
    if !autosuspend {
        machine = virtual_machines::set_auto_suspend(api_key, &machine.id, false, json)
            .await
            .with_context(|| {
                format!(
                    "Machine {} was created, but turning auto-suspend off failed. Run `stashbase vms autosuspend {} off`.",
                    machine.id, machine.id
                )
            })?;
    }
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
    } else if !silent {
        println!(
            "Virtual machine {} is being created; it is usually ready in about a minute.",
            machine.id
        );
        println!(
            "Check it with `stashbase vms list`, then connect with `stashbase vms connect {}`.",
            machine.id
        );
    }
    Ok(())
}

/// A machine as `vms list` shows it.
#[derive(Tabled)]
struct MachineRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "Region")]
    region: String,
    #[tabled(rename = "SSH")]
    ssh: String,
}

async fn list(api_key: &str, json: bool, silent: bool) -> Result<()> {
    let spinner = (!silent && !json).then(request_spinner);
    let machines = virtual_machines::list(api_key, json).await;
    drop(spinner);
    let machines = machines?;
    if json {
        println!("{}", get_formatted_json_string(&machines, true)?);
    } else if machines.is_empty() {
        println!("No virtual machines.");
    } else {
        let rows: Vec<MachineRow> = machines
            .iter()
            .filter(|m| m.state != "deleted")
            .map(|m| MachineRow {
                id: m.id.clone(),
                name: m.name.clone().unwrap_or_else(|| "---".to_owned()),
                state: state_label(m),
                region: m.location.clone().unwrap_or_else(|| "---".to_owned()),
                // Plain SSH needs your key on the machine; `stashbase vms connect <id>` does not.
                ssh: m
                    .ssh_command
                    .clone()
                    .or_else(|| m.ip.clone())
                    .unwrap_or_else(|| "---".to_owned()),
            })
            .collect();
        println!("{}", build_table(&rows));
    }
    Ok(())
}

fn hours(seconds: f64) -> String {
    let h = seconds.max(0.0) / 3600.0;
    if h < 1.0 {
        format!("{} min", (seconds.max(0.0) / 60.0).ceil() as u64)
    } else if (h - h.round()).abs() < 0.05 {
        format!("{} h", h.round() as u64)
    } else {
        format!("{h:.1} h")
    }
}

/// Minutes from now until an ISO time (negative when it has passed).
fn minutes_until(iso: &str) -> Option<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    Some((t.timestamp() - chrono::Utc::now().timestamp()) / 60)
}

fn span(minutes: i64) -> String {
    let m = minutes.max(0);
    if m >= 60 {
        format!("{} h {} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

async fn find_machine(api_key: &str, id: &str, json: bool, silent: bool) -> Result<Machine> {
    validate_id(id)?;
    let spinner = (!silent && !json).then(request_spinner);
    let machines = virtual_machines::list(api_key, json).await;
    drop(spinner);
    machines?
        .into_iter()
        .find(|m| machine_uuid(&m.id).ok() == machine_uuid(id).ok())
        .ok_or_else(|| virtual_machines::not_found(json))
}

/// Label/value lines, labels padded to one width.
fn print_fields(fields: &[(&str, String)]) {
    let width = fields.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    for (label, value) in fields {
        println!("{label:<width$}  {value}");
    }
}

async fn info(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    let machine = find_machine(api_key, id, json, silent).await?;
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
        return Ok(());
    }
    let dash = || "---".to_owned();
    let mut fields = vec![
        ("ID", machine.id.clone()),
        ("Name", machine.name.clone().unwrap_or_else(dash)),
        ("State", state_label(&machine)),
        ("Region", machine.location.clone().unwrap_or_else(dash)),
        ("IP", machine.ip.clone().unwrap_or_else(dash)),
        ("SSH", machine.ssh_command.clone().unwrap_or_else(dash)),
    ];
    if let Some(since) = &machine.running_since {
        let up = minutes_until(since)
            .map(|m| format!(" (up {})", span(-m)))
            .unwrap_or_default();
        fields.push(("Running since", format!("{since}{up}")));
    }
    fields.push((
        "Auto-suspend",
        if machine.auto_suspend_enabled == Some(false) {
            "off"
        } else {
            "on"
        }
        .to_owned(),
    ));
    if machine.state == "running" && machine.auto_suspend_enabled != Some(false) {
        fields.push((
            "Idle suspend",
            match &machine.idle_suspend_at {
                Some(at) => {
                    let left = minutes_until(at)
                        .map(|m| format!(" (in {})", span(m)))
                        .unwrap_or_default();
                    format!("{at}{left}")
                }
                None => "busy, stays on".to_owned(),
            },
        ));
    }
    if let Some(note) = state_note(&machine) {
        fields.push(("Status", note.to_owned()));
    }
    if let Some(until) = &machine.session_kept_until {
        fields.push(("Session kept until", until.clone()));
    }
    if let Some(e) = &machine.error {
        fields.push(("Error", e.clone()));
    }
    if !silent {
        println!();
    }
    print_fields(&fields);
    println!(
        "\nConnect with `stashbase vms connect {}`; resources with `stashbase vms metrics {}`.",
        machine.id, machine.id
    );
    if machine.ssh_command.is_some() {
        println!("Plain SSH needs your own key added to the machine.");
    }
    Ok(())
}

async fn metrics(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    let machine = find_machine(api_key, id, json, silent).await?;
    if json {
        println!("{}", get_formatted_json_string(&machine.metrics, true)?);
        return Ok(());
    }
    if !silent {
        println!();
    }
    let Some(m) = machine
        .metrics
        .as_ref()
        .filter(|_| machine.state == "running")
    else {
        println!(
            "{}",
            if machine.state == "running" {
                "No readings yet; the machine reports about once a minute after it starts."
            } else {
                "No readings while the machine is not running."
            }
        );
        return Ok(());
    };
    let gb = |b: f64| b / 1e9;
    let ago = minutes_until(&m.measured_at)
        .map(|a| {
            if a > -1 {
                "under a minute ago".to_owned()
            } else {
                format!("{} ago", span(-a))
            }
        })
        .unwrap_or_else(|| m.measured_at.clone());
    print_fields(&[
        ("CPU", format!("{:.0}%", m.cpu_percent)),
        (
            "Memory",
            format!(
                "{:.1} GB of {:.1} GB ({:.0}%)",
                gb(m.memory_used_bytes),
                gb(m.memory_total_bytes),
                m.memory_used_bytes / m.memory_total_bytes * 100.0
            ),
        ),
        (
            "Disk",
            format!(
                "{:.1} GB of {:.0} GB ({:.0}%)",
                gb(m.storage_used_bytes),
                gb(m.storage_total_bytes),
                m.storage_used_bytes / m.storage_total_bytes * 100.0
            ),
        ),
        ("Measured", ago),
    ]);
    Ok(())
}

/// What a suspended or stopped machine keeps, in the dashboard's words.
/// The state as shown in tables: an archived machine says so, since it takes minutes to restore.
fn state_label(machine: &Machine) -> String {
    if machine.archived == Some(true) {
        format!("{} (archived)", machine.state)
    } else {
        machine.state.clone()
    }
}

fn state_note(machine: &Machine) -> Option<&'static str> {
    match machine.state.as_str() {
        _ if machine.archived == Some(true) => Some(
            "Archived after a few days unused. Your files are kept; starting it restores its disk, which takes a few minutes.",
        ),
        "suspended" => Some("Idle. Your session is kept and resumes in seconds."),
        "stopped" => Some("Shut down. Your files are kept; programs start fresh."),
        _ => None,
    }
}

/// The spinner text for bringing a machine back: a suspended one resumes, an archived one is
/// restored (minutes), anything else starts.
fn start_message(machine: &Machine) -> &'static str {
    if machine.archived == Some(true) {
        "Restoring the machine from its archive..."
    } else if machine.state == "suspended" {
        "Resuming the machine..."
    } else {
        "Starting the machine..."
    }
}

async fn rename(api_key: &str, id: &str, name: &str, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    let name = name.trim();
    let name = (!name.is_empty()).then_some(name);
    let machine = virtual_machines::rename(api_key, id, name, json).await?;
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
    } else if !silent {
        match name {
            Some(name) => println!("Machine {id} is now named {name}."),
            None => println!("Machine {id} no longer has a name."),
        }
    }
    Ok(())
}

async fn autosuspend(
    api_key: &str,
    id: &str,
    enabled: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    validate_id(id)?;
    let machine = virtual_machines::set_auto_suspend(api_key, id, enabled, json).await?;
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
    } else if !silent {
        println!(
            "{}",
            if enabled {
                format!("Machine {id} now suspends when idle; its session is kept and resumes in seconds.")
            } else {
                format!("Machine {id} keeps running when idle. Usage limits still apply; stop it with `stashbase vms stop {id}`.")
            }
        );
    }
    Ok(())
}

async fn usage(api_key: &str, json: bool, silent: bool) -> Result<()> {
    let spinner = (!silent && !json).then(request_spinner);
    let usage = virtual_machines::usage(api_key, json).await;
    drop(spinner);
    let usage = usage?;
    if json {
        println!("{}", get_formatted_json_string(&usage, true)?);
        return Ok(());
    }
    let Some(a) = &usage.allowance else {
        println!(
            "{}",
            if usage.allowance_status == "unclaimed" {
                "No machine time yet. The workspace owner can claim the free trial in the dashboard."
            } else {
                "Machines are not available for this workspace's plan."
            }
        );
        return Ok(());
    };
    let period = match (a.kind.as_str(), a.ends_at.as_deref()) {
        ("trial", _) => "one-time trial, shared by the workspace".to_owned(),
        (_, Some(end)) => format!(
            "shared by the workspace, resets {}",
            &end[..10.min(end.len())]
        ),
        _ => "shared by the workspace".to_owned(),
    };
    println!(
        "Machine time: {} of {} used ({period})",
        hours(a.used_seconds.min(a.included_seconds)),
        hours(a.included_seconds)
    );
    println!("Time left:    {}", hours(a.remaining_seconds));
    // Shown always, as in the dashboard: past the included traffic it is billed or stops machines.
    if a.included_egress_bytes > 0.0 {
        println!(
            "Data out:     {:.1} of {:.0} GB",
            a.used_egress_bytes / 1e9,
            a.included_egress_bytes / 1e9
        );
    }
    if a.overage_cap_cents > 0.0 {
        let mut extra = Vec::new();
        if a.overage_used_seconds > 0.0 {
            extra.push(hours(a.overage_used_seconds));
        }
        if a.egress_overage_bytes > 0.0 {
            extra.push(format!("{:.1} GB", a.egress_overage_bytes / 1e9));
        }
        println!(
            "Extra usage:  ${:.2} of ${:.2}{} (billed on the next invoice)",
            a.overage_cents / 100.0,
            a.overage_cap_cents / 100.0,
            if extra.is_empty() {
                String::new()
            } else {
                format!(", {}", extra.join(" and "))
            }
        );
    }
    if let Some(m) = &usage.machines {
        println!("Your machines: {} of {}", m.current_count, m.max_count);
    }
    if usage.allowance_status == "exhausted" {
        println!("\nIncluded usage is used up; machines stop until it resets or the owner allows extra hours.");
    }
    Ok(())
}

/// Asks which machine to run on: one of the user's existing machines (with a
/// key on this computer), or a new one.
/// The region for a new machine: the one passed, or asked for (connect runs in a terminal),
/// among the locations offered, each shown with its cloud region.
async fn region(api_key: &str, location: Option<&str>, json: bool) -> Result<String> {
    if let Some(location) = location {
        return Ok(location.to_owned());
    }
    let usage = virtual_machines::usage(api_key, json).await?;
    let mut offered = usage.locations;
    if offered.is_empty() {
        offered = vec!["us".into(), "eu".into(), "asia".into()];
    }
    let labels = offered
        .iter()
        .map(|l| {
            let name = match l.as_str() {
                "us" => "United States",
                "eu" => "Europe",
                "asia" => "Asia",
                other => other,
            };
            match usage.location_regions.get(l) {
                Some(region) => format!("{name} ({region})"),
                None => name.to_owned(),
            }
        })
        .collect();
    match select(
        "Create the machine in which location? Pick the closest for a responsive terminal.",
        labels,
    ) {
        Some(index) => Ok(offered[index].clone()),
        None => bail!("Cancelled."),
    }
}

/// States `prepare_ssh` can start or wait for, so a machine that is busy for a moment is still
/// connected to rather than a second one created.
fn can_prepare_ssh(state: &str) -> bool {
    matches!(
        state,
        "running"
            | "stopped"
            | "suspended"
            | "error"
            | "creating"
            | "resuming"
            | "suspending"
            | "stopping"
    )
}

async fn pick_or_create(
    api_key: &str,
    location: Option<&str>,
    json: bool,
    silent: bool,
) -> Result<String> {
    let machines = virtual_machines::list(api_key, json).await?;
    // A machine without a key from this CLI gets one added when you connect.
    let usable: Vec<Machine> = machines
        .into_iter()
        .filter(|m| can_prepare_ssh(&m.state))
        .collect();
    // Most members have one machine: use it without asking. `vms create` makes another.
    if let [only] = usable.as_slice() {
        if !silent && !json {
            eprintln!("Connecting to {}.", only.id);
        }
        return Ok(only.id.clone());
    }
    if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        bail!(
            "Pass a machine ID, or run in a terminal to pick a machine. \
             List machines with `stashbase vms list`."
        );
    }
    if usable.is_empty() {
        if !silent {
            eprintln!("No existing virtual machines; creating a new one.");
        }
        let location = region(api_key, location, json).await?;
        return Ok(create_machine(api_key, &location, None, true, json, silent)
            .await?
            .id);
    }
    let mut labels: Vec<String> = usable
        .iter()
        .map(|m| format!("{}  {:<8}", m.id, m.state))
        .collect();
    labels.push("Create a new machine".to_string());
    match select("Run on which virtual machine?", labels) {
        Some(index) if index < usable.len() => Ok(usable[index].id.clone()),
        Some(_) => {
            let location = region(api_key, location, json).await?;
            Ok(create_machine(api_key, &location, None, true, json, silent)
                .await?
                .id)
        }
        None => bail!("Cancelled."),
    }
}

async fn run(api_key: &str, args: VirtualMachineConnect, json: bool, silent: bool) -> Result<()> {
    check_ssh_args(&args.ssh_arg)?;
    let id = match args.id {
        Some(id) => id,
        None => pick_or_create(api_key, args.location.as_deref(), json, silent).await?,
    };
    connect(
        api_key,
        &id,
        &args.command,
        &args.forward,
        &args.ssh_arg,
        args.keep_awake,
        !args.no_tmux,
        json,
        silent,
    )
    .await
}

/// Resumes a machine, waiting while another operation runs on it. A machine unused for a few days
/// is archived in the background (minutes), and the stopped machine refuses to start until that
/// ends; `connect` waits it out instead of failing. Gives up after about ten minutes.
async fn resume_when_free(api_key: &str, id: &str, json: bool) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    loop {
        match virtual_machines::resume(api_key, id, json).await {
            Err(error)
                if format!("{error:#}").contains("virtual_machine_busy")
                    && std::time::Instant::now() < deadline =>
            {
                virtual_machines::pause(Duration::from_secs(5), json).await?;
            }
            // Another `connect` or `cp` resumed it first: follow that start instead of failing.
            Err(error) if format!("{error:#}").contains("invalid_virtual_machine_state") => {
                let machines = virtual_machines::list(api_key, json).await?;
                let started = machines.iter().any(|m| {
                    machine_uuid(&m.id).ok() == machine_uuid(id).ok()
                        && matches!(m.state.as_str(), "resuming" | "running")
                });
                return if started { Ok(()) } else { Err(error) };
            }
            result => return result.map(|_| ()),
        }
    }
}

/// Follows a machine that is being created or started until it runs (up to about five minutes:
/// restoring an archived machine takes a few).
async fn wait_until_running(api_key: &str, id: &str, json: bool) -> Result<Machine> {
    timeout(Duration::from_secs(300), async {
        loop {
            if let Ok(machines) = virtual_machines::list(api_key, json).await {
                match machines
                    .into_iter()
                    .find(|m| machine_uuid(&m.id).ok() == machine_uuid(id).ok())
                {
                    // Ready means SSH answers; older backends without the flag count as ready.
                    Some(m) if m.state == "running" && m.ready != Some(false) => return Ok(m),
                    Some(m) if m.state == "running" => {}
                    Some(m) if matches!(m.state.as_str(), "creating" | "resuming") => {}
                    Some(m) => bail!(
                        "Virtual machine {id} did not start ({}). Try `stashbase vms start {id}` again.",
                        m.error.unwrap_or(m.state)
                    ),
                    None => return Err(virtual_machines::not_found(json)),
                }
            }
            virtual_machines::pause(Duration::from_secs(3), json).await?;
        }
    })
    .await
    .with_context(|| format!("Virtual machine {id} is still starting; try connecting again shortly."))?
}

/// Follows a machine that is suspending or stopping until it has (a suspend takes about 95 s).
async fn wait_until_settled(api_key: &str, id: &str, json: bool) -> Result<Machine> {
    timeout(Duration::from_secs(300), async {
        loop {
            if let Ok(machines) = virtual_machines::list(api_key, json).await {
                match machines
                    .into_iter()
                    .find(|m| machine_uuid(&m.id).ok() == machine_uuid(id).ok())
                {
                    Some(m) if matches!(m.state.as_str(), "suspending" | "stopping") => {}
                    Some(m) => return Ok(m),
                    None => return Err(virtual_machines::not_found(json)),
                }
            }
            virtual_machines::pause(Duration::from_secs(3), json).await?;
        }
    })
    .await
    .with_context(|| {
        format!("Virtual machine {id} is still stopping; try connecting again shortly.")
    })?
}

/// What `ssh` and `scp` need to reach a machine as `agent`: its address, this computer's key, a
/// fresh certificate for it, and its pinned host key.
struct SshTarget {
    ip: String,
    key: PathBuf,
    cert_path: PathBuf,
    known_hosts: PathBuf,
    /// This invocation's own folder for the certificate and pinned host key, so concurrent
    /// `connect` and `cp` runs never read a file another one is rewriting. Removed on drop.
    session_dir: PathBuf,
}

impl Drop for SshTarget {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.session_dir);
    }
}

impl SshTarget {
    /// Options shared by `ssh` and `scp`.
    fn options(&self) -> Result<Vec<std::ffi::OsString>> {
        // No config files: a `KnownHostsCommand` or other trust source from the user's or the
        // system's ssh config must not stand in for the host key the API issued.
        let mut options: Vec<std::ffi::OsString> = vec![
            "-F".into(),
            "none".into(),
            "-i".into(),
            self.key.clone().into(),
        ];
        for option in [
            "IdentitiesOnly=yes".to_string(),
            "LogLevel=ERROR".to_string(),
            // Short attempts: a first connection dropped while the machine settles costs
            // seconds, not ten; the attempts still add up to a long enough wait.
            "ConnectTimeout=3".to_string(),
            "ConnectionAttempts=30".to_string(),
            format!("CertificateFile={}", ssh_option_path(&self.cert_path)?),
            // Every invocation must negotiate a fresh connection and verify the host key.
            "ControlPath=none".to_string(),
            "StrictHostKeyChecking=yes".to_string(),
            "GlobalKnownHostsFile=/dev/null".to_string(),
            "UpdateHostKeys=no".to_string(),
            format!("UserKnownHostsFile={}", ssh_option_path(&self.known_hosts)?),
        ] {
            options.push("-o".into());
            options.push(option.into());
        }
        Ok(options)
    }
}

/// Starts the machine if it is stopped (resumes it if suspended), waits until it accepts SSH, and
/// gets a short-lived certificate for this computer's key.
async fn prepare_ssh(api_key: &str, id: &str, json: bool, silent: bool) -> Result<SshTarget> {
    validate_id(id)?;
    let dir = machine_key_dir(id)?;
    let key = dir.join("id_ed25519");
    let machines = virtual_machines::list(api_key, json).await?;
    prune_key_dirs(&machines);
    let machine = machines
        .into_iter()
        .find(|m| machine_uuid(&m.id).ok() == machine_uuid(id).ok())
        .ok_or_else(|| virtual_machines::not_found(json))?;
    // A machine still suspending or shutting down is waited for, then started like any other.
    let machine = if matches!(machine.state.as_str(), "suspending" | "stopping") {
        let message = if machine.state == "suspending" {
            "Waiting for the machine to finish suspending..."
        } else {
            "Waiting for the machine to finish stopping..."
        };
        let mut spinner = (!silent && !json).then(|| new_spinner(message, Streams::Stderr));
        let settled = wait_until_settled(api_key, id, json).await;
        if let Some(mut spinner) = spinner.take() {
            spinner.clear();
        }
        settled?
    } else {
        machine
    };
    // Starting returns at once; the machine is followed until it runs. One already being created
    // or started is waited for the same way.
    match machine.state.as_str() {
        "running" if machine.ready != Some(false) => machine,
        // Running but SSH does not answer yet (still booting): wait for it like a start.
        "running" => {
            let mut spinner = (!silent && !json).then(|| {
                new_spinner(
                    "Waiting for the machine to finish booting...",
                    Streams::Stderr,
                )
            });
            let started = wait_until_running(api_key, id, json).await;
            if let Some(mut spinner) = spinner.take() {
                spinner.clear();
            }
            started?
        }
        "stopped" | "suspended" | "error" | "creating" | "resuming" => {
            // An archived machine restores its disk first, which takes minutes: say so.
            let message = if machine.state == "creating" {
                "Creating the machine..."
            } else {
                start_message(&machine)
            };
            let mut spinner = (!silent && !json).then(|| new_spinner(message, Streams::Stderr));
            let started = async {
                if matches!(machine.state.as_str(), "stopped" | "suspended" | "error") {
                    resume_when_free(api_key, id, json).await?;
                }
                wait_until_running(api_key, id, json).await
            }
            .await;
            if let Some(mut spinner) = spinner.take() {
                spinner.clear();
            }
            started?
        }
        other => return Err(virtual_machines::busy(id, other, json)),
    };
    // Every connection uses a short-lived certificate for this computer's key, issued by the
    // backend for this machine only, and checks the machine's host key against the one it
    // reported at boot. The local key is made on the first connect to a machine.
    let public_key = load_or_create_key(&dir)?;
    remove_stale_sessions(&dir);
    let session_dir = dir.join(format!("session-{}", uuid::Uuid::new_v4()));
    let cert_path = session_dir.join("id_ed25519-cert.pub");
    let known_hosts = session_dir.join("known_hosts");
    let issued = match certificate_when_ready(api_key, id, &public_key, json, silent).await {
        Ok(issued) => issued,
        Err(error) => {
            // The key stays: another run may be using it, and the next connect reuses it.
            if format!("{error:#}").contains("host_key_unavailable") {
                bail!("The machine has not reported its host key yet. It is probably still starting; try again in a minute.");
            }
            return Err(error);
        }
    };
    fs::create_dir_all(&session_dir)?;
    fs::write(&cert_path, format!("{}\n", issued.certificate.trim()))?;
    fs::write(
        &known_hosts,
        format!("{} {}\n", issued.ip, issued.host_key.trim()),
    )?;
    Ok(SshTarget {
        ip: issued.ip,
        key,
        cert_path,
        known_hosts,
        session_dir,
    })
}

/// Removes session and key-generation folders left by runs that exited without cleaning up (an
/// exit code passed through from ssh, a crash). Their certificates expired long ago. Best effort.
fn remove_stale_sessions(dir: &Path) {
    remove_sessions_older_than(dir, Duration::from_secs(60 * 60));
}

fn remove_sessions_older_than(dir: &Path, max_age: Duration) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age > max_age);
        let name = entry.file_name().to_string_lossy().into_owned();
        if old && (name.starts_with("session-") || name.starts_with("keygen-")) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Accepts only `--ssh-arg` values known to leave the pinned host key and the certificate alone:
/// verbosity, quiet, compression, address family, and the keepalive settings. A denylist misses
/// grouped short options (`-vF file` sets a config file), so anything else is refused.
fn check_ssh_args(ssh_args: &[String]) -> Result<()> {
    for arg in ssh_args {
        let flag = matches!(
            arg.as_str(),
            "-v" | "-vv" | "-vvv" | "-q" | "-C" | "-4" | "-6"
        );
        let keepalive = arg
            .strip_prefix("-o")
            .and_then(|option| option.split_once('='))
            .is_some_and(|(name, value)| {
                (name.eq_ignore_ascii_case("ServerAliveInterval")
                    || name.eq_ignore_ascii_case("ServerAliveCountMax"))
                    && value.parse::<u32>().is_ok()
                    && value.bytes().all(|byte| byte.is_ascii_digit())
            });
        if !flag && !keepalive {
            bail!(
                "`--ssh-arg={arg}` is not supported. Allowed: -v, -vv, -vvv, -q, -C, -4, -6, \
                 -oServerAliveInterval=<seconds> and -oServerAliveCountMax=<count>."
            );
        }
    }
    Ok(())
}

/// The `ssh` options for `connect`, before the destination: the pinned host key and certificate
/// first, then the user's allowlisted extras (`check_ssh_args`), then our keepalive defaults and
/// forwards. ssh keeps the first value of a repeated option, so the extras cannot loosen the pin
/// and a user's keepalive wins over ours.
fn connection_args(
    target: &SshTarget,
    ssh_args: &[String],
    forwards: &[PortForward],
) -> Result<Vec<std::ffi::OsString>> {
    let mut args = target.options()?;
    args.extend(ssh_args.iter().map(Into::into));
    // A machine that suspends freezes the connection without closing it; notice within about
    // 45 seconds instead of hanging.
    for option in ["ServerAliveInterval=15", "ServerAliveCountMax=3"] {
        args.extend(["-o".into(), option.into()]);
    }
    // Forwards listen on loopback only, so nobody else on this network reaches the machine's
    // ports; a local port already in use fails the connection instead of only warning.
    if !forwards.is_empty() {
        args.extend(["-o".into(), "ExitOnForwardFailure=yes".into()]);
    }
    for forward in forwards {
        args.push("-L".into());
        args.push(format!("127.0.0.1:{}:localhost:{}", forward.local, forward.remote).into());
    }
    Ok(args)
}

/// Starts the machine if it is stopped (resumes it if suspended), then opens an interactive shell, or
/// runs `command` there, through the user's own `ssh`.
#[allow(clippy::too_many_arguments)]
async fn connect(
    api_key: &str,
    id: &str,
    command: &[String],
    forwards: &[PortForward],
    ssh_args: &[String],
    keep_awake: Option<Duration>,
    use_tmux: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    let target = prepare_ssh(api_key, id, json, silent).await?;
    let mut ssh = Command::new("ssh");
    ssh.args(connection_args(&target, ssh_args, forwards)?);
    // An explicit command gets a terminal only when both ends are one (an interactive program
    // such as `htop`). With output redirected it runs without one, as with plain ssh, so stderr
    // stays separate and line endings are not translated.
    if !command.is_empty() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        ssh.arg("-t");
    }
    ssh.arg(format!("agent@{}", target.ip));
    if !command.is_empty() {
        // One argument is a command line for the machine's shell, as with plain ssh
        // (`-- 'echo $HOME'`). Several are quoted one by one, so each stays a single word.
        let remote = match command {
            [line] => line.clone(),
            _ => command
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
        };
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
    // Ctrl+C belongs to ssh (and the machine); this process ignores it while ssh runs, so it is
    // still here afterwards to restore the terminal. ssh itself gets the default handling back.
    #[cfg(unix)]
    let previous_sigint = unsafe {
        use std::os::unix::process::CommandExt;
        ssh.pre_exec(|| {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            Ok(())
        });
        libc::signal(libc::SIGINT, libc::SIG_IGN)
    };
    if !silent && !json {
        for forward in forwards {
            eprintln!(
                "Forwarding localhost:{} to the machine's port {}. Traffic through it keeps the machine awake; an unused forward does not.",
                forward.local, forward.remote
            );
        }
        if let Some(limit) = keep_awake {
            eprintln!(
                "Keeping the machine awake while connected, for up to {}. Usage limits still apply.",
                format_minutes(limit)
            );
        }
    }
    // While connected, tell the backend every few minutes that someone is here, up to the limit.
    // The pings stop with this process, so a closed terminal, a crash or a sleeping laptop
    // lets the machine suspend after the normal idle time again.
    let pinger = keep_awake.map(|limit| {
        let (api_key, id) = (api_key.to_owned(), id.to_owned());
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            loop {
                if started.elapsed() >= limit {
                    return true;
                }
                // A failed ping is simply retried with the next one.
                let _ = virtual_machines::keep_awake(&api_key, &id, true).await;
                sleep(KEEP_AWAKE_INTERVAL).await;
            }
        })
    });
    // Nothing starts after a Ctrl+C during the wait, not even the ssh session it led up to.
    virtual_machines::ensure_not_aborted(json)?;
    let saved_terminal = save_terminal();
    // ssh runs on a blocking thread so the pings above keep running meanwhile.
    let status = tokio::task::spawn_blocking(move || ssh.status())
        .await
        .context("ssh did not finish")?;
    let keep_awake_ended = match pinger {
        Some(pinger) if pinger.is_finished() => pinger.await.unwrap_or(false),
        Some(pinger) => {
            pinger.abort();
            false
        }
        None => false,
    };
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGINT, previous_sigint);
    }
    let status = status.context("`ssh` was not found on this machine.")?;
    restore_terminal(saved_terminal.as_deref());
    if keep_awake_ended && !silent && !json {
        if let Some(limit) = keep_awake {
            eprintln!(
                "Keep-awake ended after {}, so the machine could suspend after the normal idle time again.",
                format_minutes(limit)
            );
        }
    }
    // 255 is ssh's own failure: here usually a connection that died because the machine was
    // suspended (idle) or stopped. Say what to do instead of leaving a bare error.
    if status.code() == Some(255) && !silent && !json {
        eprintln!(
            "The connection to the machine was lost: it was suspended while idle, stopped, or the \
             network dropped. Run `stashbase vms connect {id}` to start it again and reconnect."
        );
    }
    if !status.success() {
        // The shell's own exit code is passed through, reported the way every other command does.
        crate::exit::Exit::code(status.code().unwrap_or(1)).terminate();
    }
    Ok(())
}

/// `vm_abc:path` (or a machine UUID before the colon) names a path on a machine, and `:path` a
/// path on a machine you pick. Anything else is local; a Windows drive letter
/// (`C:\\dir`) is a single character before the colon, so it never looks like a machine.
fn split_remote(arg: &str) -> Option<(Option<&str>, &str)> {
    if let Some(path) = arg.strip_prefix(':') {
        return Some((None, path));
    }
    let (id, path) = arg.split_once(':')?;
    (id.starts_with("vm_") || uuid::Uuid::parse_str(id).is_ok()).then_some((Some(id), path))
}

/// A local `scp` operand that scp cannot mistake for `host:path`: a relative path gets `./` in
/// front, since scp only looks for a host before the first `/`.
fn local_operand(path: &str) -> std::ffi::OsString {
    if Path::new(path).is_absolute() || path.starts_with("./") || path.starts_with("../") {
        path.into()
    } else {
        Path::new(".").join(path).into_os_string()
    }
}

/// The machine `:path` means: the one you pick in a terminal, even when you have only one, so
/// files never go somewhere you did not choose. Scripts must name the machine (`ID:path`).
async fn pick_existing(api_key: &str, json: bool) -> Result<String> {
    let machines: Vec<Machine> = virtual_machines::list(api_key, json)
        .await?
        .into_iter()
        .filter(|m| !matches!(m.state.as_str(), "deleted" | "deleting"))
        .collect();
    match machines.as_slice() {
        [] => bail!("You have no virtual machines. Create one with `stashbase vms create`."),
        _ if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() => {
            bail!("Name the machine as `ID:path`. List machines with `stashbase vms list`.")
        }
        _ => {
            let labels = machines
                .iter()
                .map(|m| format!("{}  {:<8}", m.id, m.state))
                .collect();
            match select("Copy with which virtual machine?", labels) {
                Some(index) => Ok(machines[index].id.clone()),
                None => bail!("Cancelled."),
            }
        }
    }
}

/// Copies files between this computer and a machine through the user's own `scp`, with the same
/// short-lived certificate and pinned host key as `connect`.
async fn cp(
    api_key: &str,
    source: &str,
    destination: &str,
    recursive: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    let (id, remote_path, upload) = match (split_remote(source), split_remote(destination)) {
        (Some((id, path)), None) => (id, path, false),
        (None, Some((id, path))) => (id, path, true),
        (Some(_), Some(_)) => {
            bail!("Copy between this computer and one machine: only one side can be `ID:path`.")
        }
        (None, None) => {
            bail!("Name the machine side as `ID:path` or `:path`, for example `:notes.md`.")
        }
    };
    let id = match id {
        Some(id) => id.to_string(),
        None => pick_existing(api_key, json).await?,
    };
    let target = prepare_ssh(api_key, &id, json, silent).await?;
    // Relative paths start in the agent's home directory, as with plain scp.
    let remote = format!(
        "agent@{}:{}",
        target.ip,
        if remote_path.is_empty() {
            "."
        } else {
            remote_path
        }
    );
    // scp does not create missing folders on the machine: create the destination's parent first
    // (or the destination itself when it ends with `/`). A failure is left to scp to report.
    if upload {
        let parent = match remote_path.rfind('/') {
            Some(0) => None,
            Some(end) => Some(&remote_path[..end]),
            None => None,
        };
        if let Some(parent) = parent.filter(|p| !p.is_empty()) {
            let _ = Command::new("ssh")
                .args(target.options()?)
                .arg(format!("agent@{}", target.ip))
                .arg(format!("mkdir -p -- {}", shell_quote(parent)))
                .stdin(std::process::Stdio::null())
                .status();
        }
    }
    // Nothing starts after a Ctrl+C during the wait, as with `connect`.
    virtual_machines::ensure_not_aborted(json)?;
    let mut scp = Command::new("scp");
    scp.args(target.options()?);
    if recursive {
        scp.arg("-r");
    }
    if silent || json {
        scp.arg("-q");
    }
    // The local side is passed as an explicit path, so scp never reads `a:b` as a host or
    // `-x` as an option.
    scp.arg("--");
    if upload {
        scp.arg(local_operand(source)).arg(&remote);
    } else {
        scp.arg(&remote).arg(local_operand(destination));
    }
    let status = scp
        .status()
        .context("`scp` was not found on this machine.")?;
    if !status.success() {
        crate::exit::Exit::code(status.code().unwrap_or(1)).terminate();
    }
    Ok(())
}

/// How often `connect --keep-awake` pings: every 5 minutes, well within the idle time. Local
/// debug builds ping every 5 seconds, for dev backends with an idle time of a minute or two.
const KEEP_AWAKE_INTERVAL: Duration = if cfg!(debug_assertions) {
    Duration::from_secs(5)
} else {
    Duration::from_secs(300)
};

/// The terminal's settings (`stty -g`) before ssh runs, when this is an interactive terminal.
fn save_terminal() -> Option<String> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    #[cfg(unix)]
    {
        let output = Command::new("stty")
            .arg("-g")
            .stdin(std::process::Stdio::inherit())
            .output()
            .ok()?;
        let saved = String::from_utf8(output.stdout).ok()?.trim().to_owned();
        (output.status.success() && !saved.is_empty()).then_some(saved)
    }
    #[cfg(not(unix))]
    Some(String::new())
}

/// Puts the local terminal back the way it was after ssh ends. Programs on the machine (tmux,
/// agents) switch modes on (hidden cursor, mouse reporting, the alternate screen, focus reports)
/// and only switch them off when they exit cleanly; a dropped connection leaves them on. Only
/// for an interactive terminal (`saved` from `save_terminal`); the user's own settings are
/// restored, not reset.
fn restore_terminal(saved: Option<&str>) {
    use std::io::Write;
    let Some(saved) = saved else { return };
    let mut out = std::io::stdout();
    let _ = out.write_all(
        b"\x1b[?1049l\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[?2004l\x1b[0m",
    );
    let _ = out.flush();
    #[cfg(not(unix))]
    let _ = saved;
    #[cfg(unix)]
    if !saved.is_empty() {
        let _ = Command::new("stty")
            .arg(saved)
            .stdin(std::process::Stdio::inherit())
            .status();
    }
}

async fn start(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    // Listed first only to say the right thing: a suspended machine resumes, a stopped one boots.
    let current = virtual_machines::list(api_key, json)
        .await
        .ok()
        .and_then(|ms| {
            ms.into_iter()
                .find(|m| machine_uuid(&m.id).ok() == machine_uuid(id).ok())
        });
    let message = current
        .as_ref()
        .map_or("Starting the machine...", start_message);
    let mut spinner = (!silent && !json).then(|| new_spinner(message, Streams::Stderr));
    let started = virtual_machines::resume(api_key, id, json).await;
    if let Some(mut spinner) = spinner.take() {
        spinner.clear();
    }
    let machine = started?;
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
    } else if !silent {
        if current.as_ref().is_some_and(|m| m.state == "suspended") {
            println!("Virtual machine {id} is resuming; your session continues in a few seconds.");
        } else {
            println!("Virtual machine {id} is starting; it is usually ready within a minute (a few if it was archived).");
        }
        println!("Connect with: stashbase vms connect {id}");
    }
    Ok(())
}

async fn stop(api_key: &str, id: &str, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    let machine = virtual_machines::stop(api_key, id, json).await?;
    // The key stays: `run` or `ssh` starts the machine again.
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
    } else if !silent {
        println!(
            "Virtual machine {id} is shutting down; its files are kept and programs start fresh."
        );
        println!("Start it again with: stashbase vms start {id}");
    }
    Ok(())
}

async fn delete(api_key: &str, id: &str, yes: bool, json: bool, silent: bool) -> Result<()> {
    validate_id(id)?;
    // Deleting loses every file on the machine: confirm in a terminal, require --yes elsewhere.
    if !yes {
        let interactive =
            !json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
        if !interactive {
            bail!("Deleting a machine loses all its files. Pass --yes to delete without a prompt.");
        }
        eprintln!(
            "This permanently deletes virtual machine {id}, its disk and any saved snapshot. Files on it cannot be recovered."
        );
        if confirm_text("Delete machine") != "Delete machine" {
            bail!("Cancelled.");
        }
    }
    let dir = machine_key_dir(id)?;
    let machine = virtual_machines::destroy(api_key, id, json).await?;
    if let Err(error) = fs::remove_dir_all(&dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error).with_context(|| {
                format!(
                    "VM deletion was accepted, but could not remove SSH credentials at {}",
                    dir.display()
                )
            });
        }
    }
    if json {
        println!("{}", get_formatted_json_string(&machine, true)?);
    } else if !silent {
        println!("Virtual machine {id} is being deleted with its disk; it disappears from `stashbase vms list` when done.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        can_prepare_ssh, check_ssh_args, connection_args, load_or_create_key, local_operand,
        machine_key_dir, remove_sessions_older_than, split_remote, SshTarget,
    };
    use crate::cmd::virtual_machines::PortForward;
    use std::{path::PathBuf, time::Duration};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stashbase-vm-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn target(dir: &std::path::Path) -> SshTarget {
        let session_dir = dir.join(format!("session-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&session_dir).unwrap();
        SshTarget {
            ip: "192.0.2.1".into(),
            key: dir.join("id_ed25519"),
            cert_path: session_dir.join("id_ed25519-cert.pub"),
            known_hosts: session_dir.join("known_hosts"),
            session_dir,
        }
    }

    fn strings(args: Vec<std::ffi::OsString>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    /// The value of the first `-o Name=value` for `name`: the one ssh uses.
    fn first_option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
        args.windows(2).find_map(|pair| {
            let value = pair[1].strip_prefix(name)?.strip_prefix('=')?;
            (pair[0] == "-o").then_some(value)
        })
    }

    #[test]
    fn ssh_options_pin_the_issued_host_key() {
        let dir = scratch("pin");
        let target = target(&dir);
        let args = strings(target.options().unwrap());
        assert_eq!(args[..2], ["-F", "none"], "no ssh config file is read");
        assert_eq!(first_option(&args, "StrictHostKeyChecking"), Some("yes"));
        assert_eq!(first_option(&args, "ControlPath"), Some("none"));
        assert_eq!(first_option(&args, "UpdateHostKeys"), Some("no"));
        let known_hosts = format!("\"{}\"", target.known_hosts.display());
        assert_eq!(
            first_option(&args, "UserKnownHostsFile"),
            Some(known_hosts.as_str())
        );
        let certificate = format!("\"{}\"", target.cert_path.display());
        assert_eq!(
            first_option(&args, "CertificateFile"),
            Some(certificate.as_str())
        );
        assert!(target.known_hosts.starts_with(&target.session_dir));
        assert!(target.cert_path.starts_with(&target.session_dir));
        drop(target);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_keepalive_wins_but_cannot_come_before_the_pin() {
        let dir = scratch("order");
        let target = target(&dir);
        let pinned = target.options().unwrap().len();
        let args = strings(
            connection_args(
                &target,
                &["-oServerAliveInterval=60".into(), "-v".into()],
                &[PortForward {
                    local: 8080,
                    remote: 3000,
                }],
            )
            .unwrap(),
        );
        // The pinned options come first, unchanged; the user's extras right after them.
        assert_eq!(args[..pinned], strings(target.options().unwrap())[..]);
        assert_eq!(args[pinned..pinned + 2], ["-oServerAliveInterval=60", "-v"]);
        // Our keepalive defaults follow, so ssh uses the user's 60, and ours only fill gaps.
        let ours = args
            .iter()
            .position(|a| a == "ServerAliveInterval=15")
            .unwrap();
        assert!(ours > pinned + 1);
        assert_eq!(first_option(&args, "ServerAliveCountMax"), Some("3"));
        assert_eq!(first_option(&args, "ExitOnForwardFailure"), Some("yes"));
        assert!(args.ends_with(&["-L".into(), "127.0.0.1:8080:localhost:3000".into()]));
        drop(target);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn machine_paths_and_local_paths_are_told_apart() {
        let uuid = "0b9f7c4e-6a3d-4f1e-9c2b-5d8e7f6a1b2c";
        assert_eq!(
            split_remote("vm_abc:notes.md"),
            Some((Some("vm_abc"), "notes.md"))
        );
        assert_eq!(
            split_remote(&format!("{uuid}:/tmp/x")),
            Some((Some(uuid), "/tmp/x"))
        );
        assert_eq!(split_remote(":notes.md"), Some((None, "notes.md")));
        assert_eq!(split_remote(":"), Some((None, "")));
        // Local: a drive letter, a colon in a file name, no colon at all.
        assert_eq!(split_remote("C:\\dir\\file"), None);
        assert_eq!(split_remote("report:2026.txt"), None);
        assert_eq!(split_remote("notes.md"), None);
    }

    #[test]
    fn each_session_folder_goes_with_its_run_and_stale_ones_are_cleaned_up() {
        let dir = scratch("sessions");
        std::fs::write(dir.join("id_ed25519"), "key").unwrap();
        let first = target(&dir);
        let second = target(&dir);
        let first_dir = first.session_dir.clone();
        assert_ne!(first_dir, second.session_dir);
        drop(first);
        assert!(!first_dir.exists(), "a finished run removes its folder");
        assert!(second.session_dir.exists(), "another run's folder stays");

        // Folders left by runs that never cleaned up: only session and key-generation folders go,
        // never the key itself.
        std::fs::create_dir_all(dir.join("keygen-left")).unwrap();
        let leftover = second.session_dir.clone();
        std::mem::forget(second);
        remove_sessions_older_than(&dir, Duration::from_secs(3600));
        assert!(leftover.exists(), "a recent folder stays");
        std::thread::sleep(Duration::from_millis(20));
        remove_sessions_older_than(&dir, Duration::from_millis(1));
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["id_ed25519"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn connect_picks_machines_it_can_start_or_wait_for() {
        for state in [
            "running",
            "stopped",
            "suspended",
            "error",
            "creating",
            "resuming",
            "suspending",
            "stopping",
        ] {
            assert!(can_prepare_ssh(state), "{state}");
        }
        for state in ["deleting", "deleted", "unknown"] {
            assert!(!can_prepare_ssh(state), "{state}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_first_connections_share_one_key() {
        let dir = std::env::temp_dir().join(format!("stashbase-vm-key-{}", uuid::Uuid::new_v4()));
        let keys: Vec<String> = (0..4)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || load_or_create_key(&dir).unwrap())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(keys.iter().all(|key| key == &keys[0]), "{keys:?}");
        assert!(keys[0].starts_with("ssh-ed25519 "));
        // Only the key itself is left; every staging folder is gone.
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["id_ed25519"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_scp_operands_are_explicit_paths() {
        assert_eq!(local_operand("report:2026.txt"), "./report:2026.txt");
        assert_eq!(local_operand("-rf"), "./-rf");
        assert_eq!(local_operand("../out"), "../out");
        assert_eq!(local_operand("/tmp/a:b"), "/tmp/a:b");
    }

    #[test]
    fn ssh_args_cannot_replace_host_key_trust() {
        assert!(check_ssh_args(&["-v".into(), "-oServerAliveInterval=60".into()]).is_ok());
        assert!(check_ssh_args(&["-F".into()]).is_err());
        assert!(check_ssh_args(&["-F/tmp/config".into()]).is_err());
        assert!(check_ssh_args(&["-oKnownHostsCommand=/bin/echo".into()]).is_err());
        assert!(check_ssh_args(&["-vF/tmp/config".into()]).is_err());
        assert!(check_ssh_args(&["-oServerAliveInterval=-1".into()]).is_err());
        assert!(check_ssh_args(&["-oserveraliveinterval=60".into()]).is_ok());
    }

    #[test]
    fn public_and_full_machine_ids_share_the_ssh_directory() {
        let public_dir = machine_key_dir("vm_1111111111111111111112").unwrap();
        let full_dir = machine_key_dir("00000000-0000-0000-0000-000000000001").unwrap();

        assert_eq!(public_dir, full_dir);
    }
}

/// `8h`, `90m` → "8h", "1h 30m".
fn format_minutes(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}
