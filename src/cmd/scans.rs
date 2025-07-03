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
    /// Path to a baseline file; only report findings that are new compared to this baseline
    #[arg(long = "baseline", name = "baseline")]
    pub baseline: Option<String>,

    /// Relative path to a config file
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Git-like exclude patterns (files, folders) to ignore
    #[clap(value_parser, short = 'e', long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

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
    /// Path to a baseline file; only report findings that are new compared to this baseline
    #[arg(long = "baseline", name = "baseline")]
    pub baseline: Option<String>,

    /// Relative path to a config file
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Git-like exclude patterns (files, folders) to ignore
    #[clap(value_parser, short = 'e', long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

    /// Output directory
    #[arg(value_enum, short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Value hashes of secrets to ignore
    #[clap(value_parser, short = 'i', long="ignore-value-hashes", num_args = 1..)]
    pub ignore_value_hashes: Vec<String>,
}
