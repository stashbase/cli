use crate::{
    cmd::generate::{GenerateCommand, GenerateSubcommand, GenerateUuidSubcommand},
    handlers::generate::{
        random::handle_generate_random_value,
        uuid::{handle_generate_uuid_v4, handle_generate_uuid_v7},
    },
    models::generate::GenerateRandomValueAlphabet,
};
use anyhow::Result;

pub fn handle_generate_command(cmd: GenerateCommand, json_format: bool) -> Result<()> {
    match cmd.subcommand {
        GenerateSubcommand::Uuid(args) => match args.subcommand {
            GenerateUuidSubcommand::V4 => handle_generate_uuid_v4(json_format),
            GenerateUuidSubcommand::V7 => handle_generate_uuid_v7(json_format),
        },
        GenerateSubcommand::Random(args) => {
            let alphabet = GenerateRandomValueAlphabet::from(args.subcommand);
            handle_generate_random_value(
                alphabet,
                json_format,
                args.get_length(),
                args.get_uppercase(),
            )
        }
    }

    Ok(())
}
