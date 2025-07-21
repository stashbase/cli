use crate::cmd::generate::GenerateRandomSubcommand;

pub enum GenerateRandomValueAlphabet {
    Alphanumeric,
    Hexadecimal,
    Base64,
    Base64Url,
}

impl From<GenerateRandomSubcommand> for GenerateRandomValueAlphabet {
    fn from(value: GenerateRandomSubcommand) -> Self {
        match value {
            GenerateRandomSubcommand::Alphanumeric(_) => GenerateRandomValueAlphabet::Alphanumeric,
            GenerateRandomSubcommand::Hex(_) => GenerateRandomValueAlphabet::Hexadecimal,
            GenerateRandomSubcommand::Base64(_) => GenerateRandomValueAlphabet::Base64,
            GenerateRandomSubcommand::Base64Url(_) => GenerateRandomValueAlphabet::Base64Url,
        }
    }
}
