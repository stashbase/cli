use clap::Args;

#[derive(Debug, Args)]
#[command(override_usage = "scan [OPTIONS]")]
pub struct ScanCommand {
    /// Scan only selected files
    #[clap(value_parser, long="files", num_args = 1..)]
    pub files: Vec<String>,

    /// Scan only git staged files
    #[arg(value_enum, long = "print")]
    pub staged: bool,

    /// Autofix found issues
    #[arg(value_enum, long = "autofix")]
    pub autofix: bool,

    /// Save the result in the cloud
    #[arg(value_enum, long = "remote")]
    pub remote: bool,

    /// Project context
    #[arg(value_enum, short = 'p', long = "project")]
    pub project: Option<String>,

    /// Environment context
    #[arg(value_enum, short = 'e', long = "environment")]
    pub environment: Option<String>,
}
