use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud <COMMAND> [OPTIONS]")]
pub struct CloudCommand {
    #[clap(subcommand)]
    pub subcommand: CloudSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CloudSubcommand {
    /// Create a cloud machine
    Create(CloudCreate),
    /// List your cloud machines
    List(CloudList),
    /// Run on a cloud machine: pick an existing one or create a new one
    Run(CloudRun),
    /// Open a shell on a cloud machine over SSH
    Ssh(CloudSsh),
    /// Stop a cloud machine, keeping its disk; `run` or `ssh` starts it again
    Stop(CloudStop),
    /// Delete a cloud machine and its disk for good
    Delete(CloudDelete),
    /// Prepare the shared machine image (workspace owner; once per deployment)
    Setup(CloudSetup),
    /// Set the workspace machine profile from a TOML file (workspace owner)
    Profile(CloudProfile),
}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud setup")]
pub struct CloudSetup {}

#[derive(Debug, Args)]
#[command(override_usage = "agent cloud profile <FILE>")]
pub struct CloudProfile {
    /// TOML profile with egress_hosts, deny_hosts and secret bindings
    #[arg(value_name = "FILE")]
    pub file: std::path::PathBuf,
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
