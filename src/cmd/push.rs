use clap::Args;

use super::pull::PullFormat;

pub type PushFormat = PullFormat;

#[derive(Debug, Args)]
#[command(override_usage = "push  [OPTIONS]")]
pub struct PushCommand {
    /// Relative path to a config file (default: stashbase.yaml)
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Target file path if not specified in the config
    #[arg(long = "file")]
    pub file: Option<String>,

    /// Target file format (autodetected by default)
    #[arg(value_enum, long = "format")]
    pub format: Option<PushFormat>,

    /// Select secret names
    #[clap(long="only", num_args = 1..)]
    pub only: Vec<String>,

    /// Exclude secret names
    #[clap(long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

    /// Manually set secrets
    #[clap(long="set", num_args = 1..)]
    pub set: Vec<String>,

    /// Expand references to their values
    #[arg(long = "expand-refs")]
    pub expand_refs: Option<bool>,

    /// Ignore secret comments
    #[arg(long = "ignore-comments")]
    pub ignore_comments: Option<bool>,
}
