use clap::Args;

#[derive(Debug, Args)]
pub struct LoadCommand {
    /// Command to run
    pub command: String,

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

    /// Print loaded secrets
    #[arg(value_enum, long = "print")]
    pub print_secrets: bool,
}
