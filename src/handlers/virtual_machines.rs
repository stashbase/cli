//! `stashbase vms create|list|info|metrics|autosuspend|usage|connect|start|stop|delete`: managed
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
                json,
                silent,
            )
            .await
        }
        VirtualMachineSubcommand::List(_) => list(&api_key, json, silent).await,
        VirtualMachineSubcommand::Info(args) => info(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Metrics(args) => metrics(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Autosuspend(args) => {
            autosuspend(&api_key, &args.id, args.mode == "on", json, silent).await
        }
        VirtualMachineSubcommand::Usage(_) => usage(&api_key, json, silent).await,
        VirtualMachineSubcommand::Connect(args) => run(&api_key, args, json, silent).await,
        VirtualMachineSubcommand::Start(args) => start(&api_key, &args.id, json, silent).await,
        VirtualMachineSubcommand::Stop(args) => stop(&api_key, &args.id, json, silent).await,
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
    // Keep the existing directory so saved SSH keys and known-hosts files remain usable.
    Ok(dirs.data_local_dir().join("cloud"))
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
            "stashbase-virtual-machine",
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
                    sleep(Duration::from_secs(5)).await;
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

/// Creates a machine and keeps its key locally. The server answers once the machine is recorded
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
    let keys = keys_dir()?;
    let pending = keys.join(format!("pending-{}", uuid::Uuid::new_v4()));
    let public_key = generate_key(&pending)?;
    let mut spinner =
        (!silent && !json).then(|| new_spinner("Creating the virtual machine...", Streams::Stderr));
    let mut result = virtual_machines::create(api_key, &public_key, location, name, json)
        .await
        .map_err(|error| {
            if error.downcast_ref::<reqwest::Error>().is_some() {
                error.context(
                    "Could not confirm whether the virtual machine was created. Check `stashbase vms list` before retrying.",
                )
            } else {
                error
            }
        });
    // The key belongs to the machine as soon as it is recorded.
    if let Ok(machine) = &result {
        fs::rename(&pending, machine_key_dir(&machine.id)?)?;
    }
    if wait {
        if let Ok(created) = &result {
            if created.state == "creating"
                || (created.state == "running" && created.ready == Some(false))
            {
                let id = created.id.clone();
                result = timeout(Duration::from_secs(240), async {
                    loop {
                        sleep(Duration::from_secs(3)).await;
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
    match result {
        Ok(machine) => Ok(machine),
        Err(error) => {
            let _ = fs::remove_dir_all(&pending);
            Err(error)
        }
    }
}

async fn create(
    api_key: &str,
    location: Option<&str>,
    name: Option<&str>,
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
            asked = input_optional("Name (optional, press Enter to skip)");
            Some(asked.trim()).filter(|n| !n.is_empty())
        }
        None => None,
    };
    let machine = create_machine(api_key, &location, name, false, json, silent).await?;
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
                state: m.state.clone(),
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
        ("State", machine.state.clone()),
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
    if let Some(note) = state_note(&machine.state) {
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
fn state_note(state: &str) -> Option<&'static str> {
    match state {
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

async fn pick_or_create(
    api_key: &str,
    location: Option<&str>,
    json: bool,
    silent: bool,
) -> Result<String> {
    if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        bail!(
            "Pass a machine ID, or run in a terminal to pick a machine. \
             List machines with `stashbase vms list`."
        );
    }
    let machines = virtual_machines::list(api_key, json).await?;
    // A machine without a key from this CLI gets one added when you connect.
    let usable: Vec<Machine> = machines
        .into_iter()
        .filter(|m| matches!(m.state.as_str(), "running" | "stopped" | "suspended"))
        .collect();
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
    let id = match args.id {
        Some(id) => id,
        None => pick_or_create(api_key, args.location.as_deref(), json, silent).await?,
    };
    connect(
        api_key,
        &id,
        &args.command,
        &args.forward,
        args.keep_awake,
        !args.no_tmux,
        json,
        silent,
    )
    .await
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
            sleep(Duration::from_secs(3)).await;
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
            sleep(Duration::from_secs(3)).await;
        }
    })
    .await
    .with_context(|| {
        format!("Virtual machine {id} is still stopping; try connecting again shortly.")
    })?
}

/// Starts the machine if it is stopped (resumes it if suspended), then opens an interactive shell, or
/// runs `command` there, through the user's own `ssh`.
#[allow(clippy::too_many_arguments)]
async fn connect(
    api_key: &str,
    id: &str,
    command: &[String],
    forwards: &[PortForward],
    keep_awake: Option<Duration>,
    use_tmux: bool,
    json: bool,
    silent: bool,
) -> Result<()> {
    validate_id(id)?;
    let dir = machine_key_dir(id)?;
    let key = dir.join("id_ed25519");
    let machines = virtual_machines::list(api_key, json).await?;
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
                    virtual_machines::resume(api_key, id, json).await?;
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
    // reported at boot. A machine launched elsewhere (for example from the dashboard without a
    // key) gets a key made here first.
    let had_key = key.is_file();
    if !had_key {
        generate_key(&dir)?;
    }
    let public_key = read_public_key(&dir)?;
    let cert_path = dir.join("id_ed25519-cert.pub");
    let known_hosts = dir.join("known_hosts");
    let issued = match certificate_when_ready(api_key, id, &public_key, json, silent).await {
        Ok(issued) => issued,
        Err(error) => {
            if !had_key {
                let _ = fs::remove_dir_all(&dir);
            }
            if format!("{error:#}").contains("host_key_unavailable") {
                bail!("The machine has not reported its host key yet. It is probably still starting; try again in a minute.");
            }
            return Err(error);
        }
    };
    fs::write(&cert_path, format!("{}\n", issued.certificate.trim()))?;
    fs::write(
        &known_hosts,
        format!("{} {}\n", issued.ip, issued.host_key.trim()),
    )?;
    let mut ssh = Command::new("ssh");
    ssh.arg("-i")
        .arg(&key)
        .args(["-o", "IdentitiesOnly=yes"])
        .args(["-o", "LogLevel=ERROR"])
        // Short attempts: a first connection dropped while the machine settles costs seconds, not
        // ten; the attempts still add up to a long enough wait.
        .args(["-o", "ConnectTimeout=3"])
        .args(["-o", "ConnectionAttempts=30"])
        // A machine that suspends freezes the connection without closing it; notice within about
        // 45 seconds instead of hanging.
        .args(["-o", "ServerAliveInterval=15"])
        .args(["-o", "ServerAliveCountMax=3"])
        .arg("-o")
        .arg(format!("CertificateFile={}", ssh_option_path(&cert_path)?))
        // Every invocation must negotiate a fresh connection and verify the host key.
        .args(["-o", "ControlPath=none"])
        .args(["-o", "StrictHostKeyChecking=yes"])
        .args(["-o", "GlobalKnownHostsFile=/dev/null"])
        .args(["-o", "UpdateHostKeys=no"])
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile={}",
            ssh_option_path(&known_hosts)?
        ));
    // Forwards listen on loopback only, so nobody else on this network reaches the machine's
    // ports; a local port already in use fails the connection instead of only warning.
    if !forwards.is_empty() {
        ssh.args(["-o", "ExitOnForwardFailure=yes"]);
    }
    for forward in forwards {
        ssh.arg("-L").arg(format!(
            "127.0.0.1:{}:localhost:{}",
            forward.local, forward.remote
        ));
    }
    if !command.is_empty() && std::io::stdin().is_terminal() {
        ssh.arg("-t");
    }
    ssh.arg(format!("agent@{}", issued.ip));
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
                sleep(Duration::from_secs(300)).await;
                if started.elapsed() >= limit {
                    return true;
                }
                // A failed ping is retried with the next one; the idle time is far longer.
                let _ = virtual_machines::keep_awake(&api_key, &id, true).await;
            }
        })
    });
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
    restore_terminal();
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

/// Puts the local terminal back to normal after ssh ends. Programs on the machine (tmux, agents)
/// switch modes on (hidden cursor, mouse reporting, the alternate screen, focus reports) and only
/// switch them off when they exit cleanly; a dropped connection leaves them on.
fn restore_terminal() {
    use std::io::{IsTerminal, Write};
    let mut out = std::io::stdout();
    if !out.is_terminal() {
        return;
    }
    let _ = out.write_all(
        b"\x1b[?1049l\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[?2004l\x1b[0m",
    );
    let _ = out.flush();
    #[cfg(unix)]
    let _ = Command::new("stty")
        .arg("sane")
        .stdin(std::process::Stdio::inherit())
        .status();
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
    use super::machine_key_dir;

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
