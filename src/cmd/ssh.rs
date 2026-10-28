use clap::{Args, Subcommand, ValueEnum};

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
    /// Check the CLI on a machine over SSH: its version, API key and Docker
    Doctor(SshDoctor),
    /// Remove the CLI and its stored API keys from a machine over SSH
    Uninstall(SshUninstall),
    /// Connect to a machine and attach to its `stashbase` tmux session
    Open(SshOpen),
    /// List the terminal sessions on a machine that `ssh open` can attach to
    Sessions(SshSessions),
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh install <HOST> [OPTIONS]")]
pub struct SshInstall {
    /// Machine to install on, as you would pass it to `ssh` (a host from
    /// ~/.ssh/config or user@host)
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Proceed without confirmation
    #[arg(long = "yes", short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh setup <HOST>")]
pub struct SshSetup {
    /// Machine to set up, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh doctor <HOST>")]
pub struct SshDoctor {
    /// Machine to check, as you would pass it to `ssh`
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

#[derive(Debug, Args)]
#[command(override_usage = "ssh open <HOST> [OPTIONS]")]
pub struct SshOpen {
    /// Machine to connect to, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Terminal multiplexer whose session to attach to, or create
    #[arg(long = "with", value_enum, default_value = "tmux")]
    pub multiplexer: Multiplexer,

    /// Session to attach to, or create; another name keeps work apart from
    /// the default session
    #[arg(long = "session", value_name = "NAME", default_value = "stashbase")]
    pub session: String,

    /// Make a port on the machine, such as a dev server, reachable on this
    /// one while connected: `3000`, or `8080:3000` for local port 8080.
    /// Repeatable
    #[arg(long = "port", value_name = "PORT")]
    pub ports: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh sessions <HOST> [OPTIONS]")]
pub struct SshSessions {
    /// Machine to list the sessions of, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Terminal multiplexer whose sessions to list
    #[arg(long = "with", value_enum, default_value = "tmux")]
    pub multiplexer: Multiplexer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Multiplexer {
    Tmux,
    Screen,
    Zellij,
    /// A plain login shell
    None,
}
