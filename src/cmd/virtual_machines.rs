use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "vms <COMMAND> [OPTIONS]")]
pub struct VirtualMachineCommand {
    #[clap(subcommand)]
    pub subcommand: VirtualMachineSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum VirtualMachineSubcommand {
    /// Create a virtual machine
    Create(VirtualMachineCreate),
    /// List your virtual machines
    List(VirtualMachineList),
    /// Show a machine's details and how to connect
    Info(VirtualMachineInfo),
    /// Show a running machine's CPU, memory and disk
    Metrics(VirtualMachineMetrics),
    /// Rename a virtual machine (an empty name clears it)
    Rename(VirtualMachineRename),
    /// Turn suspending when idle on or off for a machine
    Autosuspend(VirtualMachineAutosuspend),
    /// Show the workspace's machine time and data transfer for this period
    Usage(VirtualMachineUsage),
    /// Connect to a virtual machine: your only one, or pick or create one
    Connect(VirtualMachineConnect),
    /// Copy files between this computer and a machine (`ID:path` or `:path` names the machine side)
    Cp(VirtualMachineCp),
    /// Start a stopped virtual machine without connecting to it
    #[command(visible_alias = "resume")]
    Start(VirtualMachineStart),
    /// Stop a virtual machine, keeping its disk; `start` or `connect` starts it again
    Stop(VirtualMachineStop),
    /// Delete a virtual machine and its disk for good
    Delete(VirtualMachineDelete),
}

#[derive(Debug, Args)]
#[command(override_usage = "vms create [--location <us|eu|asia>] [--name <NAME>]")]
pub struct VirtualMachineCreate {
    /// A name for the machine (up to 40 letters, numbers, spaces, dots, dashes or underscores).
    /// Asked for in a terminal when omitted; leave it empty to skip.
    #[arg(long)]
    pub name: Option<String>,

    /// Where to create the machine: us (United States), eu (Europe) or asia (Asia). Pick the
    /// closest for a responsive terminal. Omitted in a terminal, you pick from the offered
    /// locations with their cloud regions; required in scripts.
    #[arg(long, value_parser = ["us", "eu", "asia"])]
    pub location: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms list")]
pub struct VirtualMachineList {}

#[derive(Debug, Args)]
#[command(override_usage = "vms info <ID>")]
pub struct VirtualMachineInfo {
    /// Machine ID (vm_… or legacy UUID)
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms metrics <ID>")]
pub struct VirtualMachineMetrics {
    /// Machine ID (vm_… or legacy UUID)
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms rename <ID> <NAME>")]
pub struct VirtualMachineRename {
    /// Machine ID (vm_… or legacy UUID)
    #[arg(value_name = "ID")]
    pub id: String,
    /// The new name (up to 40 letters, numbers, spaces, dots, dashes or underscores); `""` clears it
    #[arg(value_name = "NAME")]
    pub name: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms autosuspend <ID> <on|off>")]
pub struct VirtualMachineAutosuspend {
    /// Machine ID (vm_… or legacy UUID)
    #[arg(value_name = "ID")]
    pub id: String,
    /// `on` suspends the machine after a period without activity (the session is kept); `off`
    /// keeps it running
    #[arg(value_name = "on|off", value_parser = ["on", "off"])]
    pub mode: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms usage")]
pub struct VirtualMachineUsage {}

#[derive(Debug, Args)]
#[command(
    override_usage = "vms connect [ID] [--location us|eu|asia] [--forward <[LOCAL:]PORT>]... [--keep-awake[=DURATION]] [--no-tmux] [--ssh-arg <ARG>]... [-- <COMMAND>...]"
)]
pub struct VirtualMachineConnect {
    /// Machine ID (vm_… or legacy UUID) to use; omit to pick an existing one or create a new one
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// Where a machine created here goes (when no ID is given): us, eu or asia; asked for when
    /// omitted
    #[arg(long, value_parser = ["us", "eu", "asia"])]
    pub location: Option<String>,

    /// Forward a port on the machine to this computer while connected, e.g. `3000` (localhost:3000
    /// opens the machine's port 3000) or `8080:3000` (localhost:8080 opens its port 3000).
    /// Only this computer can use it. Repeat for more ports.
    #[arg(long, value_name = "[LOCAL:]PORT", value_parser = parse_forward)]
    pub forward: Vec<PortForward>,

    /// Keep the machine from suspending while idle for as long as this stays connected, up to
    /// DURATION (default 8h, at most 24h; e.g. `--keep-awake=2h`, `--keep-awake=90m`). Usage
    /// limits still apply. Disconnecting ends it.
    #[arg(
        long,
        value_name = "DURATION",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "8h",
        value_parser = parse_keep_awake
    )]
    pub keep_awake: Option<std::time::Duration>,

    /// Open a plain login shell instead of the machine's shared tmux session
    #[arg(long)]
    pub no_tmux: bool,

    /// Pass an extra argument to `ssh`, e.g. `--ssh-arg=-v` to debug a connection or
    /// `--ssh-arg=-oServerAliveInterval=60`. Repeat for more. The machine's host key checks and
    /// certificate always apply, and the machine only allows local port forwards.
    #[arg(long, value_name = "ARG", allow_hyphen_values = true)]
    pub ssh_arg: Vec<String>,

    /// Attach to the machine's shared tmux session (the default)
    #[arg(long, conflicts_with = "no_tmux")]
    pub tmux: bool,

    /// Command to run on the machine instead of an interactive shell
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms start <ID>")]
pub struct VirtualMachineStart {
    /// Machine ID (vm_… or legacy UUID) to start
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms stop <ID>")]
pub struct VirtualMachineStop {
    /// Machine ID (vm_… or legacy UUID) to stop
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms delete <ID> [--yes]")]
pub struct VirtualMachineDelete {
    /// Machine ID (vm_… or legacy UUID) to delete
    #[arg(value_name = "ID")]
    pub id: String,

    /// Delete without asking for confirmation (required in scripts)
    #[arg(long = "yes", short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Args)]
#[command(
    override_usage = "vms cp [-r] <SOURCE> <DESTINATION>",
    after_help = "Examples:\n  stashbase vms cp ./notes.md :notes.md\n  stashbase vms cp -r :project/dist ./dist\n  stashbase vms cp ./notes.md vm_abc123:notes.md\n\n`:path` uses your only machine, or asks which one when you have several. Paths on the machine are relative to its home directory (/home/agent) unless they start with /."
)]
pub struct VirtualMachineCp {
    /// What to copy: a local path, or `ID:path` / `:path` on a machine
    #[arg(value_name = "SOURCE")]
    pub source: String,

    /// Where to copy it: a local path, or `ID:path` / `:path` on a machine. Exactly one side is a machine.
    #[arg(value_name = "DESTINATION")]
    pub destination: String,

    /// Copy directories and their contents
    #[arg(short = 'r', long)]
    pub recursive: bool,
}

/// A port on the machine (`remote`) reachable on this computer's `local` port while connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortForward {
    pub local: u16,
    pub remote: u16,
}

fn parse_forward(value: &str) -> Result<PortForward, String> {
    let port = |p: &str| match p.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(format!("`{p}` is not a port (1-65535)")),
    };
    match value.split_once(':') {
        Some((local, remote)) => Ok(PortForward {
            local: port(local)?,
            remote: port(remote)?,
        }),
        None => {
            let port = port(value)?;
            Ok(PortForward {
                local: port,
                remote: port,
            })
        }
    }
}

/// `90m`, `2h`, `1h30m`: between one minute and 24 hours.
fn parse_keep_awake(value: &str) -> Result<std::time::Duration, String> {
    let invalid = || format!("`{value}` is not a duration such as 30m, 2h or 1h30m");
    let (mut minutes, mut number) = (0u64, String::new());
    for c in value.trim().chars() {
        match c {
            '0'..='9' => number.push(c),
            'h' | 'm' => {
                let n: u64 = number.parse().map_err(|_| invalid())?;
                minutes = minutes.saturating_add(if c == 'h' { n.saturating_mul(60) } else { n });
                number.clear();
            }
            _ => return Err(invalid()),
        }
    }
    if !number.is_empty() || minutes == 0 {
        return Err(invalid());
    }
    if minutes > 24 * 60 {
        return Err("--keep-awake is at most 24h".into());
    }
    Ok(std::time::Duration::from_secs(minutes * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keep_awake() {
        assert_eq!(parse_keep_awake("90m").unwrap().as_secs(), 90 * 60);
        assert_eq!(parse_keep_awake("2h").unwrap().as_secs(), 2 * 3600);
        assert_eq!(parse_keep_awake("1h30m").unwrap().as_secs(), 90 * 60);
        for bad in ["", "0m", "2", "h", "2d", "25h", "-1h"] {
            assert!(parse_keep_awake(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_forwards() {
        assert_eq!(
            parse_forward("3000"),
            Ok(PortForward {
                local: 3000,
                remote: 3000
            })
        );
        assert_eq!(
            parse_forward("8080:3000"),
            Ok(PortForward {
                local: 8080,
                remote: 3000
            })
        );
        for bad in ["", "0", "70000", "abc", "1:", ":1", "1:2:3"] {
            assert!(parse_forward(bad).is_err(), "{bad}");
        }
    }
}
