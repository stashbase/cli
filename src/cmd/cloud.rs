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
    /// Open a shell in a cloud session over SSH
    Ssh(CloudSsh),
    /// Stop a cloud session and destroy its machine
    Stop(CloudStop),
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
pub struct CloudTunnel {
    #[arg(value_name = "ID")]
    pub id: String,
}
