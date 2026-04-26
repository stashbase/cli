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

    /// Generate SSH key pair
    #[clap(name = "ssh-keypair", alias = "ssh")]
    SshKeypair(GenerateSshKeypair),
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
    pub fn get_length(&self) -> Option<usize> {
        match &self.subcommand {
            GenerateRandomStringSubcommand::Alphanumeric(options) => {
                options.length.map(|v| v as usize)
            }
            GenerateRandomStringSubcommand::Hex(options) => options.length.map(|v| v as usize),
            GenerateRandomStringSubcommand::Base32(options) => options.length.map(|v| v as usize),
            GenerateRandomStringSubcommand::Base64(options) => options.length.map(|v| v as usize),
            GenerateRandomStringSubcommand::Base64Url(options) => {
                options.length.map(|v| v as usize)
            }
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

    pub fn get_effective_bytes(&self) -> Option<u16> {
        match (self.get_bytes(), self.get_length()) {
            (Some(bytes), _) => Some(bytes),
            (None, None) => Some(32),
            (None, Some(_)) => None,
        }
    }

    pub fn get_target_length(&self) -> usize {
        let bytes = self.get_effective_bytes();

        if let Some(bytes) = bytes {
            match &self.subcommand {
                GenerateRandomStringSubcommand::Hex(_) => (bytes as usize) * 2,
                GenerateRandomStringSubcommand::Alphanumeric(_) => {
                    ((bytes as f64) * 8.0 / (62.0f64).log2()).ceil() as usize
                }
                GenerateRandomStringSubcommand::Base32(_) => {
                    ((bytes as f64) * 8.0 / 5.0).ceil() as usize
                }
                GenerateRandomStringSubcommand::Base64(_)
                | GenerateRandomStringSubcommand::Base64Url(_) => {
                    ((bytes as f64) * 4.0 / 3.0).ceil() as usize
                }
            }
        } else {
            self.get_length().unwrap_or(32)
        }
    }
}

#[derive(Debug, Args)]
pub struct GenerateRandomOptions {
    /// Length of the random string
    #[arg(short = 'l', long = "length", value_parser = clap::value_parser!(u16).range(3..=256), conflicts_with = "bytes")]
    pub length: Option<u16>,

    /// Desired entropy length in bytes
    #[arg(short = 'b', long = "bytes", value_parser = clap::value_parser!(u16).range(3..=256), conflicts_with = "length")]
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

#[derive(Debug, Args)]
#[command(override_usage = "generate ssh-keypair [OPTIONS]")]
pub struct GenerateSshKeypair {
    /// SSH key algorithm
    #[arg(short = 't', long = "type", value_enum, default_value = "ed25519")]
    pub key_type: GenerateSshKeyType,

    /// Key size in bits (RSA only, default: 4096)
    #[arg(short = 'b', long = "bits", value_parser = clap::value_parser!(u16).range(2048..=16384))]
    pub bits: Option<u16>,

    /// Key comment
    #[arg(short = 'c', long = "comment", default_value = "stashbase@local")]
    pub comment: String,

    /// Private key output path
    #[arg(short = 'o', long = "out", default_value = "~/.ssh/id_stashbase")]
    pub out: String,

    /// Passphrase for private key (empty by default)
    #[arg(long = "passphrase")]
    pub passphrase: Option<String>,

    /// Overwrite existing key files
    #[arg(long = "force")]
    pub force: bool,

    /// Print generated public key content
    #[arg(long = "print-public")]
    pub print_public: bool,
}

#[derive(Debug, ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum GenerateSshKeyType {
    Ed25519,
    Rsa,
}

impl GenerateSshKeyType {
    pub fn as_ssh_keygen_type(&self) -> &'static str {
        match self {
            GenerateSshKeyType::Ed25519 => "ed25519",
            GenerateSshKeyType::Rsa => "rsa",
        }
    }
}
