use crate::cmd::generate::GenerateRandomSubcommand;

pub enum GenerateRandomStringAlphabet {
    Alphanumeric,
    Hexadecimal,
    Base64,
    Base64Url,
}

impl From<GenerateRandomSubcommand> for GenerateRandomStringAlphabet {
    fn from(value: GenerateRandomSubcommand) -> Self {
        match value {
            GenerateRandomSubcommand::Alphanumeric(_) => GenerateRandomStringAlphabet::Alphanumeric,
            GenerateRandomSubcommand::Hex(_) => GenerateRandomStringAlphabet::Hexadecimal,
            GenerateRandomSubcommand::Base64(_) => GenerateRandomStringAlphabet::Base64,
            GenerateRandomSubcommand::Base64Url(_) => GenerateRandomStringAlphabet::Base64Url,
        }
    }
}
