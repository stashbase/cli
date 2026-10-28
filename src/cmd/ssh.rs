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
    /// List or close the terminal sessions on a machine that `ssh open` attaches to
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
#[command(override_usage = "ssh sessions <COMMAND> [OPTIONS]")]
pub struct SshSessions {
    #[clap(subcommand)]
    pub subcommand: SshSessionsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SshSessionsSubcommand {
    /// List the sessions on a machine that `ssh open --session` attaches to
    List(SshSessionsList),
    /// Close a session on a machine, stopping everything running in it
    Close(SshSessionsClose),
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh sessions list <HOST> [OPTIONS]")]
pub struct SshSessionsList {
    /// Machine to list the sessions of, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Terminal multiplexer whose sessions to list
    #[arg(long = "with", value_enum, default_value = "tmux")]
    pub multiplexer: Multiplexer,
}

#[derive(Debug, Args)]
#[command(override_usage = "ssh sessions close <HOST> <NAME> [OPTIONS]")]
pub struct SshSessionsClose {
    /// Machine the session is on, as you would pass it to `ssh`
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Session to close, as `ssh sessions list` shows it
    #[arg(value_name = "NAME")]
    pub name: String,

    /// Terminal multiplexer the session belongs to
    #[arg(long = "with", value_enum, default_value = "tmux")]
    pub multiplexer: Multiplexer,

    /// Proceed without confirmation
    #[arg(long = "yes", short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Multiplexer {
    Tmux,
    Screen,
    Zellij,
    /// A plain login shell
    None,
}
