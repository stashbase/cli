use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "ssh <COMMAND> [OPTIONS]")]
pub struct SshCommand {
    #[clap(subcommand)]
    pub subcommand: SshSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SshSubcommand {
    /// Install the CLI on a machine over SSH
    Install(SshInstall),
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh install <HOST> [OPTIONS]")]
pub struct SshInstall {
    /// Machine to install on, as you would pass it to `ssh` (a host from
    /// ~/.ssh/config or user@host)
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Install only; don't log the machine in
    #[arg(long = "no-login")]
    pub no_login: bool,
}
