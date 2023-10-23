use clap::Args;

#[derive(Debug, Args)]
pub struct LoadCommand {
    /// Command to run
    pub command: String,

    /// Project name
    #[arg(value_enum, short = 'p', long = "project", required = true)]
    pub project: String,

    /// Enviornment name
    #[arg(value_enum, short = 'e', long = "environment", required = true)]
    pub environment: String,
}
