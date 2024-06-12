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

    /// Print cli state
    Print,
}

#[derive(Debug, Args)]
pub struct SetState {
    #[arg(value_enum, short = 'p', long = "project", required = false)]
    pub project: Option<String>,

    /// Environment name
    #[arg(value_enum, short = 'e', long = "environment", required = false)]
    pub environment: Option<String>,
}
