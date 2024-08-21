use clap::Args;

use super::pull::PullFormat;

pub type PushFormat = PullFormat;

#[derive(Debug, Args)]
#[command(override_usage = "push  [OPTIONS]")]
pub struct PushCommand {
    /// Relative path to a config file (default: stashbase.yaml)
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Target file path if not specified in the config
    #[arg(value_enum, long = "file")]
    pub file: Option<String>,

    /// Target file format (autodetected by default)
    #[arg(value_enum, long = "format")]
    pub format: Option<PullFormat>,

    /// Select secret keys
    #[clap(value_parser, long="only", num_args = 1..)]
    pub only: Vec<String>,

    /// Exclude secret keys
    #[clap(value_parser, long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

    /// Manually set secrets
    #[clap(value_parser, long="set", num_args = 1..)]
    pub set: Vec<String>,
    //
    ///// Expand references to their values
    //#[arg(value_enum, long = "expand-refs")]
    //pub expand_refs: Option<bool>,
}
