use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "generate <COMMAND> [OPTIONS]")]
pub struct GenerateCommand {
    #[clap(subcommand)]
    pub subcommand: GenerateSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum GenerateSubcommand {
    Uuid(GenerateUuid),
    Random(GenerateRandomString),
}

#[derive(Debug, Args)]
#[command(override_usage = "generate uuid [OPTIONS]")]
pub struct GenerateUuid {
    #[clap(subcommand)]
    pub subcommand: GenerateUuidSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum GenerateUuidSubcommand {
    V4,
    V7,
}

#[derive(Debug, Args)]
#[command(override_usage = "generate random <TYPE> [OPTIONS]")]
pub struct GenerateRandomString {
    #[clap(subcommand)]
    pub subcommand: GenerateRandomStringSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum GenerateRandomStringSubcommand {
    Alphanumeric(GenerateRandomOptions),
    Hex(GenerateRandomOptions),
    Base64(GenerateRandomOptions),
    Base64Url(GenerateRandomOptions),
}

impl GenerateRandomString {
    pub fn get_length(&self) -> usize {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => options.length,
            GenerateRandomStringSubcommand::Hex(options) => options.length,
            GenerateRandomStringSubcommand::Base64(options) => options.length,
            GenerateRandomStringSubcommand::Base64Url(options) => options.length,
        }
    }

    pub fn get_uppercase(&self) -> bool {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => options.uppercase,
            GenerateRandomStringSubcommand::Hex(options) => options.uppercase,
            GenerateRandomStringSubcommand::Base64(options) => options.uppercase,
            GenerateRandomStringSubcommand::Base64Url(options) => options.uppercase,
        }
    }
}

#[derive(Debug, Args)]
pub struct GenerateRandomOptions {
    /// Length of the random string, defaults to 32
    #[arg(long = "length", default_value = "32")]
    pub length: usize,

    /// Include uppercase letters
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}
