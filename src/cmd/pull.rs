use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};

use super::config::SecretsOutputFormat;

#[derive(Serialize, Deserialize, Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum PullFormat {
    #[serde(rename = "dotenv")]
    Dotenv,
    #[serde(rename = "yaml")]
    Yaml,
    #[serde(rename = "json")]
    Json,
}

impl TryFrom<PullFormat> for SecretsOutputFormat {
    type Error = ();

    fn try_from(pf: PullFormat) -> Result<SecretsOutputFormat, Self::Error> {
        match pf {
            PullFormat::Dotenv => Ok(SecretsOutputFormat::Dotenv),
            PullFormat::Yaml => Ok(SecretsOutputFormat::Yaml),
            PullFormat::Json => Ok(SecretsOutputFormat::Json),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Args)]
#[command(override_usage = "pull [OPTIONS]")]
pub struct PullCommand {
    /// Relative path to a config file (default: env-ease.yaml)
    #[arg(value_enum, short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// Output file path if not specified in the config
    #[arg(value_enum, short = 'o', long = "output")]
    pub output_file: Option<String>,

    /// Format output
    #[arg(value_enum, long = "format")]
    pub format: Option<PullFormat>,

    // /// Project name
    // #[arg(value_enum, short = 'p', long = "project")]
    // pub project: Option<String>,
    //
    // /// Enviornment name
    // #[arg(value_enum, short = 'e', long = "environment")]
    // pub environment: Option<String>,
    /// Select secret keys
    #[clap(value_parser, long="only", num_args = 1..)]
    pub only: Vec<String>,

    /// Exclude secret keys
    #[clap(value_parser, long="exclude", num_args = 1..)]
    pub exclude: Vec<String>,

    /// Manually set secrets
    #[clap(value_parser, long="set", num_args = 1..)]
    pub set: Vec<String>,

    /// Replace refereces with referred values
    #[arg(value_enum, long = "replace-refs")]
    pub replace_refs: Option<bool>,

    /// Print loaded secrets
    #[arg(value_enum, long = "print")]
    pub print_secrets: bool,
}
