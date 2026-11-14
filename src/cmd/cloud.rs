use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "vms <COMMAND> [OPTIONS]")]
pub struct CloudCommand {
    #[clap(subcommand)]
    pub subcommand: CloudSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CloudSubcommand {
    /// Create a virtual machine
    Create(CloudCreate),
    /// List your virtual machines
    List(CloudList),
    /// Connect to a virtual machine: pick an existing one or create a new one
    Connect(CloudConnect),
    /// Stop a virtual machine, keeping its disk; `connect` starts it again
    Stop(CloudStop),
    /// Delete a virtual machine and its disk for good
    Delete(CloudDelete),
    /// Prepare the shared machine image (workspace owner; once per deployment)
    Setup(CloudSetup),
    /// Set the workspace machine profile from a TOML file (workspace owner)
    Profile(CloudProfile),
}

#[derive(Debug, Args)]
#[command(override_usage = "vms setup [--rebuild]")]
pub struct CloudSetup {
    /// Build a new image even though one is ready (for example after the API address changed)
    #[arg(long)]
    pub rebuild: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms profile <FILE>")]
pub struct CloudProfile {
    /// TOML profile with egress_hosts, deny_hosts and secret bindings
    #[arg(value_name = "FILE")]
    pub file: std::path::PathBuf,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms create")]
pub struct CloudCreate {}

#[derive(Debug, Args)]
#[command(override_usage = "vms list")]
pub struct CloudList {}

#[derive(Debug, Args)]
#[command(override_usage = "vms connect [ID] [--no-tmux] [-- <COMMAND>...]")]
pub struct CloudConnect {
    /// Machine ID (vm_… or legacy UUID) to use; omit to pick an existing one or create a new one
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// Open a plain login shell instead of the machine's shared tmux session
    #[arg(long)]
    pub no_tmux: bool,

    /// Attach to the machine's shared tmux session (the default)
    #[arg(long, conflicts_with = "no_tmux")]
    pub tmux: bool,

    /// Command to run on the machine instead of an interactive shell
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms stop <ID>")]
pub struct CloudStop {
    /// Machine ID (vm_… or legacy UUID) to stop
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms delete <ID>")]
pub struct CloudDelete {
    /// Machine ID (vm_… or legacy UUID) to delete
    #[arg(value_name = "ID")]
    pub id: String,
}
