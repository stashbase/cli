use clap::{Args, Subcommand, ValueEnum};

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

    /// Generate hash from value
    Hash(GenerateHash),

    /// Generate random passphrase
    #[clap(alias = "phrase")]
    Passphrase(GeneratePassphrase),
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
    #[arg(short = 'l', long = "length", default_value = "32", value_parser = clap::value_parser!(u16).range(3..=256))]
    pub length: u16,

    /// Desired entropy length in bytes (overrides --length)
    #[arg(short = 'b', long = "bytes", value_parser = clap::value_parser!(u16).range(3..=256))]
    pub bytes: Option<u16>,

    /// Make the random string uppercase
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}

#[derive(Debug, Args)]
#[command(override_usage = "generate hash <VALUE> [OPTIONS]")]
pub struct GenerateHash {
    /// Value to hash
    #[arg(value_name = "VALUE")]
    pub value: String,

    /// Hash algorithm
    #[arg(short = 'a', long = "algorithm", value_enum, default_value = "sha256")]
    pub algorithm: GenerateHashAlgorithm,

    /// Make hash uppercase
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
pub enum GenerateHashAlgorithm {
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

#[derive(Debug, Args)]
#[command(override_usage = "generate passphrase [OPTIONS]")]
pub struct GeneratePassphrase {
    /// Number of words in passphrase
    #[arg(short = 'w', long = "words", default_value = "6", value_parser = clap::value_parser!(u8).range(3..=24))]
    pub words: u8,

    /// Separator between words
    #[arg(short = 's', long = "separator", default_value = "-")]
    pub separator: String,

    /// Make passphrase uppercase
    #[arg(long = "uppercase")]
    pub uppercase: bool,
}
