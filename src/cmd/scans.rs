use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
#[command(override_usage = "scan <COMMAND> [OPTIONS]")]
pub struct ScanCommands {
    #[clap(subcommand)]
    pub subcommand: ScanSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ScanSubcommand {
    /// Scan staged files to be committed
    #[clap(alias = "pre-commit")]
    Staged(ScanStaged),

    /// Scan commits to be pushed to remote
    #[clap(alias = "pre-push")]
    Commits(ScanCommits),
}

#[derive(Debug, Args)]
#[command(override_usage = "scan staged [OPTIONS]")]
pub struct ScanStaged {}

#[derive(Debug, Args)]
#[command(override_usage = "scan commits [OPTIONS]")]
pub struct ScanCommits {}
