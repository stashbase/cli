use crate::cmd::generate::GenerateRandomStringSubcommand;

pub enum Encoding {
    Alphanumeric,
    Hex,
    Base64,
    Base64Url,
}

impl From<GenerateRandomStringSubcommand> for Encoding {
    fn from(value: GenerateRandomStringSubcommand) -> Self {
        match value {
            GenerateRandomStringSubcommand::Hex(_) => Encoding::Hex,
            GenerateRandomStringSubcommand::Base64(_) => Encoding::Base64,
            GenerateRandomStringSubcommand::Base64Url(_) => Encoding::Base64Url,
            GenerateRandomStringSubcommand::Alphanumeric(_) => Encoding::Alphanumeric,
        }
    }
}
