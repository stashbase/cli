use crate::{
    cmd::secrets::{SecretArgs, SecretSubcommand, SecretsFromat},
    handlers::secrets::{
        delete::{handle_delete_secrets, HandleDeleteSecretsArgs},
        description::{handle_update_description, HandleDescriptionArgs},
        get::{handle_get_secrets, HandleGetSecretsArgs},
        list::{handle_list_secrets, HandleListSecretsArgs},
        set::{handle_set_secrets, HandleSetSecretsArgs},
        upload::{handle_upload_secrets, HandleUploadSecretsArgs},
    },
};

pub async fn handle_secrets_commands(cmd: SecretArgs, token: String, raw_output: bool) {
    match cmd.subcommand {
        SecretSubcommand::List(args) => {
            let args = HandleListSecretsArgs {
                token,
                only_keys: args.only_keys,
                project: cmd.project,
                environment: cmd.environment,
                format: args.format.unwrap_or(if raw_output {
                    SecretsFromat::Json
                } else {
                    SecretsFromat::List
                }),
            };

            handle_list_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Get(args) => {
            let args = HandleGetSecretsArgs {
                token,
                project: cmd.project,
                environment: cmd.environment,
                keys: args.keys,
                format: args.format.unwrap_or(if raw_output {
                    SecretsFromat::Json
                } else {
                    SecretsFromat::List
                }),
            };

            handle_get_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Delete(args) => {
            let args = HandleDeleteSecretsArgs {
                token,
                project: cmd.project,
                environment: cmd.environment,
                keys: args.keys,
                delete_all: args.delete_all,
            };

            handle_delete_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Set(args) => {
            let args = HandleSetSecretsArgs {
                token,
                project: cmd.project,
                environment: cmd.environment,
                values: args.secrets,
                description: args.descriptions,
            };

            handle_set_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Description(args) => {
            let args = HandleDescriptionArgs {
                token,
                project: cmd.project,
                environment: cmd.environment,
                description: args.description,
                key: args.key,
            };

            handle_update_description(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Upload(args) => {
            let args = HandleUploadSecretsArgs {
                token,
                project: cmd.project,
                environment: cmd.environment,
                file_path: args.file_path,
            };

            handle_upload_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
    }
}
