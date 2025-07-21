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
    Random(GenerateRandom),
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
pub struct GenerateRandom {
    #[clap(subcommand)]
    pub subcommand: GenerateRandomSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum GenerateRandomSubcommand {
    Alphanumeric(GenerateRandomOptions),
    Hex(GenerateRandomOptions),
    Base64(GenerateRandomOptions),
    Base64Url(GenerateRandomOptions),
}

impl GenerateRandom {
    pub fn get_length(&self) -> usize {
        match &self.subcommand {
            GenerateRandomSubcommand::Alphanumeric(options) => options.length,
            GenerateRandomSubcommand::Hex(options) => options.length,
            GenerateRandomSubcommand::Base64(options) => options.length,
            GenerateRandomSubcommand::Base64Url(options) => options.length,
        }
    }

    pub fn get_uppercase(&self) -> bool {
        match &self.subcommand {
            GenerateRandomSubcommand::Alphanumeric(options) => options.uppercase,
            GenerateRandomSubcommand::Hex(options) => options.uppercase,
            GenerateRandomSubcommand::Base64(options) => options.uppercase,
            GenerateRandomSubcommand::Base64Url(options) => options.uppercase,
        }
    }
}

#[derive(Debug, Args)]
pub struct GenerateRandomOptions {
    /// Length of the random string, defaults to 32
    #[arg(long = "length", default_value = "32")]
    pub length: u32,

    /// Include uppercase letters
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}
