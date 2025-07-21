use crate::cmd::generate::GenerateRandomStringSubcommand;

pub enum GenerateRandomStringAlphabet {
    Alphanumeric,
    Hex,
    Base64,
    Base64Url,
}

impl From<GenerateRandomStringSubcommand> for GenerateRandomStringAlphabet {
    fn from(value: GenerateRandomStringSubcommand) -> Self {
        match value {
            GenerateRandomStringSubcommand::Alphanumeric(_) => {
                GenerateRandomStringAlphabet::Alphanumeric
            }
            GenerateRandomStringSubcommand::Hex(_) => GenerateRandomStringAlphabet::Hex,
            GenerateRandomStringSubcommand::Base64(_) => GenerateRandomStringAlphabet::Base64,
            GenerateRandomStringSubcommand::Base64Url(_) => GenerateRandomStringAlphabet::Base64Url,
        }
    }
}
