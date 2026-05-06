use clap::{Args, Subcommand};

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

    /// Scan working directory changes (staged + unstaged)
    #[clap(alias = "diff")]
    Changes(ScanChanges),

    /// Scan unpushed commits (commits not yet pushed to remote)
    #[clap(alias = "pre-push")]
    Unpushed(ScanCommits),

    /// Install git hook for automatic scan checks
    Install(ScanInstall),

    /// Uninstall git hook block for scan checks
    Uninstall(ScanUninstall),

    /// Manage scan config files
    Config(ScanConfigCommand),
}

#[derive(Debug, Args)]
#[command(override_usage = "scan install <hook>")]
pub struct ScanInstall {
    /// Hook to install
    #[arg(value_parser = ["pre-commit", "pre-push"])]
    pub hook: String,

    /// Custom hook file path to install into (e.g. .husky/pre-commit)
    #[arg(long = "file")]
    pub file: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "scan uninstall <hook>")]
pub struct ScanUninstall {
    /// Hook to uninstall from
    #[arg(value_parser = ["pre-commit", "pre-push"])]
    pub hook: String,

    /// Custom hook file path to uninstall from (e.g. .husky/pre-commit)
    #[arg(long = "file")]
    pub file: Option<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "scan config <COMMAND>")]
pub struct ScanConfigCommand {
    #[clap(subcommand)]
    pub subcommand: ScanConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ScanConfigSubcommand {
    /// Create a starter scan config file
    Init(ScanConfigInit),
}

#[derive(Debug, Args)]
#[command(override_usage = "scan config init [OPTIONS]")]
pub struct ScanConfigInit {
    /// Path to write the scan config file
    #[arg(long = "file")]
    pub file: Option<String>,

    /// Overwrite existing config file if it already exists
    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "scan staged [OPTIONS]")]
pub struct ScanStaged {
    /// Path to a baseline file; only report findings that are new compared to this baseline
    #[arg(long = "baseline", name = "baseline")]
    pub baseline: Option<String>,

    /// Relative path to the config file
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Git-like patterns of files and folders to not scan
    #[clap(long="exclude-files", num_args = 1..)]
    pub exclude_files: Vec<String>,

    /// Output directory for the scan results
    #[arg(short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Hashes of secret values to ignore
    #[clap(long="ignore-secret-hashes", num_args = 1..)]
    pub ignore_secret_hashes: Vec<String>,

    /// Regexes of secret values to ignore
    #[clap(long="ignore-secret-regexes", num_args = 1..)]
    pub ignore_secret_regexes: Vec<String>,

    /// Project to find matched secret values
    #[clap(long = "match-project")]
    pub match_project: Option<String>,

    /// Environments to find matched secret values in the specified project; defaults to all environments in the project
    ///
    /// Environments can be selected by:
    /// - Name
    /// - ID
    /// - Folder (e.g. `folder-*`)
    #[clap(long = "match-environments", num_args = 1..)]
    pub match_environments: Vec<String>,

    /// Local files with secrets (like .env) to find matched secret values
    #[clap(long = "match-files", num_args = 1..)]
    pub match_files: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "scan changes [OPTIONS]")]
pub struct ScanChanges {
    /// Path to a baseline file; only report findings that are new compared to this baseline
    #[arg(long = "baseline", name = "baseline")]
    pub baseline: Option<String>,

    /// Relative path to the config file
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Git-like patterns of files and folders to not scan
    #[clap(long="exclude-files", num_args = 1..)]
    pub exclude_files: Vec<String>,

    /// Output directory for the scan results
    #[arg(short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Hashes of secret values to ignore
    #[clap(long="ignore-secret-hashes", num_args = 1..)]
    pub ignore_secret_hashes: Vec<String>,

    /// Regexes of secret values to ignore
    #[clap(long="ignore-secret-regexes", num_args = 1..)]
    pub ignore_secret_regexes: Vec<String>,

    /// Project to find matched secret values
    #[clap(long = "match-project")]
    pub match_project: Option<String>,

    /// Environments to find matched secret values in the specified project; defaults to all environments in the project
    ///
    /// Environments can be selected by:
    /// - Name
    /// - ID
    /// - Folder (e.g. `folder-*`)
    #[clap(long = "match-environments", num_args = 1..)]
    pub match_environments: Vec<String>,

    /// Local files with secrets (like .env) to find matched secret values
    #[clap(long = "match-files", num_args = 1..)]
    pub match_files: Vec<String>,
}

#[derive(Debug, Args)]
#[command(override_usage = "scan unpushed [OPTIONS]")]
pub struct ScanCommits {
    /// Number of commits to scan from the most recent commit (default: all)
    #[clap(value_name = "N", long = "last", value_parser = clap::value_parser!(u32).range(1..=1000),)]
    pub last_n_commits: Option<u32>,
    /// Path to a baseline file; only report findings that are new compared to this baseline
    #[arg(long = "baseline", name = "baseline")]
    pub baseline: Option<String>,

    /// Relative path to the config file
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Git-like patterns of files and folders to not scan
    #[clap(long="exclude-files", num_args = 1..)]
    pub exclude_files: Vec<String>,

    /// Output directory for the scan results
    #[arg(short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Hashes of secret values to ignore
    #[clap(long="ignore-secret-hashes", num_args = 1..)]
    pub ignore_secret_hashes: Vec<String>,

    /// Regexes of secret values to ignore
    #[clap(long="ignore-secret-regexes", num_args = 1..)]
    pub ignore_secret_regexes: Vec<String>,

    /// Project to find matched secret values
    #[clap(long = "match-project")]
    pub match_project: Option<String>,

    /// Environments to find matched secret values in the specified project; defaults to all environments in the project
    ///
    /// Environments can be selected by:
    /// - Name
    /// - ID
    /// - Folder (e.g. `folder-*`)
    #[clap(long = "match-environments", num_args = 1..)]
    pub match_environments: Vec<String>,

    /// Local files with secrets (like .env) to find matched secret values
    #[clap(long = "match-files", num_args = 1..)]
    pub match_files: Vec<String>,
}
