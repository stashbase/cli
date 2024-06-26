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

    /// Select state from 'stasthbase.yaml'
    Select(SelectState),

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

#[derive(Debug, Args)]
#[command(override_usage = "state select [OPTIONS]")]
pub struct SelectState {
    /// Relative path to a config file (default: stashbase.yaml)
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,
}
