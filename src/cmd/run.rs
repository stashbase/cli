use clap::Args;

#[derive(Debug, Args)]
#[command(override_usage = "run [OPTIONS] [COMMAND]...")]
pub struct RunCommand {
    /// Command to run
    #[clap(value_parser, num_args = 1..)]
    pub command: Vec<String>,

    /// Relative path to a config file (default: env-ease.yaml)
    #[arg(value_enum, long = "file")]
    pub file: Option<String>,

    /// Project name
    #[arg(value_enum, short = 'p', long = "project")]
    pub project: Option<String>,

    /// Enviornment name
    #[arg(value_enum, short = 'e', long = "environment")]
    pub environment: Option<String>,

    /// Select secret keys
    #[clap(value_parser, long="only", num_args = 1..)]
    pub only: Vec<String>,

    /// Exclude secret keys
    #[clap(value_parser, long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

    /// Manually set secrets
    #[clap(value_parser, long="set", num_args = 1..)]
    pub set: Vec<String>,

    /// Replace refereces with their values
    #[arg(value_enum, long = "replace-refs")]
    pub replace_refs: Option<bool>,

    /// Print loaded secrets
    #[arg(value_enum, long = "print")]
    pub print_secrets: bool,
}
