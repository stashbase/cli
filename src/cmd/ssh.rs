use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "ssh <COMMAND> [OPTIONS]")]
pub struct SshCommand {
    #[clap(subcommand)]
    pub subcommand: SshSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SshSubcommand {
    /// Install the CLI on a machine over SSH, or update it
    Install(SshInstall),
    /// Run `stashbase setup` on a machine over SSH
    Setup(SshSetup),
    /// Remove the CLI and its stored API keys from a machine over SSH
    Uninstall(SshUninstall),
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh install <HOST> [OPTIONS]")]
pub struct SshInstall {
    /// Machine to install on, as you would pass it to `ssh` (a host from
    /// ~/.ssh/config or user@host)
    #[arg(value_name = "HOST")]
    pub host: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh setup <HOST>")]
pub struct SshSetup {
    /// Machine to set up, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh uninstall <HOST> [OPTIONS]")]
pub struct SshUninstall {
    /// Machine to remove the CLI from, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Proceed without confirmation
    #[arg(long = "yes", short = 'y')]
    pub yes: bool,
}
