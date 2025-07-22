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
    #[clap(alias = "rand")]
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
#[command(override_usage = "generate random <ENCODING> [OPTIONS]")]
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

    /// Generate random base32 string
    Base32(GenerateRandomOptions),

    /// Generate random base64 string
    Base64(GenerateRandomOptions),

    /// Generate random base64url string
    #[clap(name = "base64url")]
    Base64Url(GenerateRandomOptions),
}

impl GenerateRandomString {
    pub fn get_length(&self) -> usize {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => options.length as usize,
            GenerateRandomStringSubcommand::Hex(options) => options.length as usize,
            GenerateRandomStringSubcommand::Base32(options) => options.length as usize,
            GenerateRandomStringSubcommand::Base64(options) => options.length as usize,
            GenerateRandomStringSubcommand::Base64Url(options) => options.length as usize,
        }
    }

    pub fn get_uppercase(&self) -> bool {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => options.uppercase,
            GenerateRandomStringSubcommand::Hex(options) => options.uppercase,
            GenerateRandomStringSubcommand::Base32(options) => options.uppercase,
            GenerateRandomStringSubcommand::Base64(options) => options.uppercase,
            GenerateRandomStringSubcommand::Base64Url(options) => options.uppercase,
        }
    }

    pub fn get_bytes(&self) -> Option<u16> {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => options.bytes,
            GenerateRandomStringSubcommand::Hex(options) => options.bytes,
            GenerateRandomStringSubcommand::Base32(options) => options.bytes,
            GenerateRandomStringSubcommand::Base64(options) => options.bytes,
            GenerateRandomStringSubcommand::Base64Url(options) => options.bytes,
        }
    }

    pub fn get_target_length(&self) -> usize {
        let bytes = self.get_bytes();

        if let Some(bytes) = bytes {
            match self.subcommand {
                GenerateRandomStringSubcommand::Hex(_) => (bytes as usize) * 2,
                GenerateRandomStringSubcommand::Alphanumeric(_) => bytes as usize,
                GenerateRandomStringSubcommand::Base32(_) => {
                    ((bytes as f64) * 8.0 / 5.0).ceil() as usize
                }
                GenerateRandomStringSubcommand::Base64(_)
                | GenerateRandomStringSubcommand::Base64Url(_) => {
                    ((bytes as f64) * 4.0 / 3.0).ceil() as usize
                }
            }
        } else {
            self.get_length()
        }
    }
}

#[derive(Debug, Args)]
pub struct GenerateRandomOptions {
    /// Length of the random string
    #[arg(short = 'l', long = "length", default_value = "32", value_parser = clap::value_parser!(u16).range(1..=256))]
    pub length: u16,

    /// Desired entropy length in bytes (overrides --length)
    #[arg(short = 'b', long = "bytes", value_parser = clap::value_parser!(u16).range(1..=256))]
    pub bytes: Option<u16>,

    /// Make the random string uppercase
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}
