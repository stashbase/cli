use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(override_usage = "generate <COMMAND> [OPTIONS]")]
pub struct GenerateCommand {
    #[clap(subcommand)]
    pub subcommand: GenerateSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum GenerateSubcommand {
    /// Generate UUID
    Uuid(GenerateUuid),

    /// Generate random string
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
    /// Generate UUID v4
    V4,

    /// Generate UUID v7
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
    /// Generate random alphanumeric string
    Alphanumeric(GenerateRandomOptions),

    /// Generate random hexadecimal string
    Hex(GenerateRandomOptions),

    /// Generate random base64 string
    Base64(GenerateRandomOptions),

    /// Generate random base64url string
    Base64Url(GenerateRandomOptions),
}

impl GenerateRandomString {
    pub fn get_length(&self) -> usize {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => options.length as usize,
            GenerateRandomStringSubcommand::Hex(options) => options.length as usize,
            GenerateRandomStringSubcommand::Base64(options) => options.length as usize,
            GenerateRandomStringSubcommand::Base64Url(options) => options.length as usize,
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
    #[arg(long = "length", default_value = "32", value_parser = clap::value_parser!(u16).range(1..=256))]
    pub length: u16,

    /// Make the random string uppercase
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}
