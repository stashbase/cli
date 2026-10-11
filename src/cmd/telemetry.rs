use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "config telemetry <COMMAND>")]
pub struct TelemetryCommand {
    #[clap(subcommand)]
    pub subcommand: TelemetrySubcommand,
}

#[derive(Debug, Subcommand, Clone, Copy)]
pub enum TelemetrySubcommand {
    /// Enable privacy-preserving telemetry
    Enable,
    /// Disable privacy-preserving telemetry
    Disable,
    /// Show whether telemetry is enabled, and why
    Status,
}
