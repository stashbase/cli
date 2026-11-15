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
    /// Connect to a virtual machine: pick an existing one or create a new one
    Connect(VirtualMachineConnect),
    /// Stop a virtual machine, keeping its disk; `connect` starts it again
    Stop(VirtualMachineStop),
    /// Delete a virtual machine and its disk for good
    Delete(VirtualMachineDelete),
    /// Prepare the shared machine image (workspace owner; once per deployment)
    Setup(VirtualMachineSetup),
}

#[derive(Debug, Args)]
#[command(override_usage = "vms setup [--rebuild]")]
pub struct VirtualMachineSetup {
    /// Build a new image even though one is ready (for example after the API address changed)
    #[arg(long)]
    pub rebuild: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "vms create")]
pub struct VirtualMachineCreate {}

#[derive(Debug, Args)]
#[command(override_usage = "vms list")]
pub struct VirtualMachineList {}

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
