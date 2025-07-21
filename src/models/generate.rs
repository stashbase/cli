use crate::cmd::generate::GenerateRandomStringSubcommand;

pub enum Encoding {
    Alphanumeric,
    Hex,
    Base32,
    Base64,
    Base64Url,
}

impl From<GenerateRandomStringSubcommand> for Encoding {
    fn from(value: GenerateRandomStringSubcommand) -> Self {
        match value {
            GenerateRandomStringSubcommand::Hex(_) => Encoding::Hex,
            GenerateRandomStringSubcommand::Base32(_) => Encoding::Base32,
            GenerateRandomStringSubcommand::Base64(_) => Encoding::Base64,
            GenerateRandomStringSubcommand::Base64Url(_) => Encoding::Base64Url,
            GenerateRandomStringSubcommand::Alphanumeric(_) => Encoding::Alphanumeric,
        }
    }
}

impl Encoding {
    pub fn get_alphabet(&self) -> &str {
        match self {
            Encoding::Hex => "0123456789abcdef",
            Encoding::Base32 => "abcdefghijklmnopqrstuvwxyz234567",
            Encoding::Base64 => "ABCDEFGHIJKLMNOPQRSTUVXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
            Encoding::Base64Url => {
                "ABCDEFGHIJKLMNOPQRSTUVXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
            }
            Encoding::Alphanumeric => {
                "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            }
        }
    }
}
