use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud <COMMAND> [OPTIONS]")]
pub struct CloudCommand {
    #[clap(subcommand)]
    pub subcommand: CloudSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CloudSubcommand {
    /// Create a cloud session
    Create(CloudCreate),
    /// List your active cloud sessions
    List(CloudList),
    /// Run on a cloud machine: pick an existing one or create a new one
    Run(CloudRun),
    /// Open a shell in a cloud session over SSH
    Ssh(CloudSsh),
    /// Stop a cloud session's machine; `run` or `ssh` starts it again
    Stop(CloudStop),
    /// Delete a cloud session and its machine for good
    Delete(CloudDelete),
    /// SSH `ProxyCommand` helper; used by `cloud ssh`
    #[command(hide = true)]
    Tunnel(CloudTunnel),
}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud create")]
pub struct CloudCreate {}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud list")]
pub struct CloudList {}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud run [ID] [-- <COMMAND>...]")]
pub struct CloudRun {
    /// Cloud session to use; omit to pick an existing one or create a new one
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// Command to run on the machine instead of an interactive shell
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud ssh <ID>")]
pub struct CloudSsh {
    /// Cloud session to connect to
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud stop <ID>")]
pub struct CloudStop {
    /// Cloud session to stop
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud delete <ID>")]
pub struct CloudDelete {
    /// Cloud session to delete
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(Debug, Args)]
pub struct CloudTunnel {
    #[arg(value_name = "ID")]
    pub id: String,
}
