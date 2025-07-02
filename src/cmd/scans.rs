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
pub struct ScanStaged {
    /// Relative path to a config file
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Output directory
    #[arg(value_enum, short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Value hashes of secrets to ignore
    #[clap(value_parser, short = 'i', long="ignore-value-hashes", num_args = 1..)]
    pub ignore_value_hashes: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "scan commits [OPTIONS]")]
pub struct ScanCommits {
    /// Relative path to a config file
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Output directory
    #[arg(value_enum, short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Value hashes of secrets to ignore
    #[clap(value_parser, short = 'i', long="ignore-value-hashes", num_args = 1..)]
    pub ignore_value_hashes: Vec<String>,
}
