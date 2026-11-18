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
    /// Show the workspace's machine time and data transfer for this period
    Usage(VirtualMachineUsage),
    /// Connect to a virtual machine: pick an existing one or create a new one
    Connect(VirtualMachineConnect),
    /// Start a stopped virtual machine without connecting to it
    #[command(visible_alias = "resume")]
    Start(VirtualMachineStart),
    /// Stop a virtual machine, keeping its disk; `start` or `connect` starts it again
    Stop(VirtualMachineStop),
    /// Delete a virtual machine and its disk for good
    Delete(VirtualMachineDelete),
}

#[derive(Debug, Args)]
#[command(override_usage = "vms create")]
pub struct VirtualMachineCreate {}

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
#[command(override_usage = "vms usage")]
pub struct VirtualMachineUsage {}

#[derive(Debug, Args)]
#[command(override_usage = "vms connect [ID] [--no-tmux] [-- <COMMAND>...]")]
pub struct VirtualMachineConnect {
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
#[command(override_usage = "vms delete <ID>")]
pub struct VirtualMachineDelete {
    /// Machine ID (vm_… or legacy UUID) to delete
    #[arg(value_name = "ID")]
    pub id: String,
}
