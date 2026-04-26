use crate::{
    cmd::generate::{GenerateCommand, GenerateSubcommand, GenerateUuidSubcommand},
    handlers::generate::{
        hash::handle_generate_hash,
        passphrase::handle_generate_passphrase,
        random::handle_generate_random_string,
        ssh_keypair::handle_generate_ssh_keypair,
        uuid::{handle_generate_uuid_v4, handle_generate_uuid_v7},
    },
    models::generate::Encoding,
};
use anyhow::Result;

pub fn handle_generate_command(cmd: GenerateCommand, json_format: bool) -> Result<()> {
    match cmd.subcommand {
        GenerateSubcommand::Uuid(args) => match args.subcommand {
            GenerateUuidSubcommand::V4 => handle_generate_uuid_v4(json_format),
            GenerateUuidSubcommand::V7 => handle_generate_uuid_v7(json_format),
        },
        GenerateSubcommand::Random(args) => {
            let length = args.get_target_length();
            let bytes = args.get_effective_bytes();
            let uppercase = args.get_uppercase();
            let alphabet = Encoding::from(args.subcommand);

            handle_generate_random_string(alphabet, json_format, length, bytes, uppercase)?
        }
        GenerateSubcommand::Hash(args) => {
            handle_generate_hash(args.value, args.algorithm, json_format, args.uppercase)
        }
        GenerateSubcommand::Passphrase(args) => {
            handle_generate_passphrase(args.words, args.separator, json_format, args.uppercase)
        }
        GenerateSubcommand::SshKeypair(args) => handle_generate_ssh_keypair(args, json_format)?,
    }

    Ok(())
}
