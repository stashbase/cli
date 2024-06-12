use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "config <COMMAND> [OPTIONS]")]
pub struct StateCommand {
    #[clap(subcommand)]
    pub subcommand: StateSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum StateSubcommand {
    /// Set cli state
    Set(SetState),

    /// Unset cli state
    Unset(UnsetState),

    /// Print cli state
    Print,
}

#[derive(Debug, Args)]
#[command(override_usage = "state set [OPTIONS]")]
pub struct SetState {
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "state unset [OPTIONS]")]
pub struct UnsetState {
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: bool,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = false)]
    pub environment: bool,
}
