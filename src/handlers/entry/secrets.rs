use crate::{
    cmd::secrets::{SecretArgs, SecretSubcommand, SecretsFromat},
    handlers::secrets::{
        delete::{handle_delete_secrets, HandleDeleteSecretsArgs},
        description::{handle_update_description, HandleDescriptionArgs},
        get::{handle_get_secrets, HandleGetSecretsArgs},
        list::{handle_list_secrets, HandleListSecretsArgs},
        rename::{handle_rename_secrets, HandleRenameSecretsArgs},
        set::{handle_set_secrets, HandleSetSecretsArgs},
        upload::{handle_upload_secrets, HandleUploadSecretsArgs},
    },
};

pub async fn handle_secrets_commands(cmd: SecretArgs, api_key: String, raw_output: bool) {
    let project_environment_result = cmd.try_get_project_environment();

    if let Err(err) = project_environment_result {
        eprintln!("{:?}", err);
        return;
    }

    let (project, environment) = project_environment_result.unwrap();

    match cmd.subcommand {
        SecretSubcommand::List(args) => {
            let args = HandleListSecretsArgs {
                api_key,
                project,
                environment,
                only_keys: args.only_keys,
                search: args.search,
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
                api_key,
                project,
                environment,
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
                api_key,
                project,
                environment,
                keys: args.keys,
                delete_all: args.delete_all,
            };

            handle_delete_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Set(args) => {
            let args = HandleSetSecretsArgs {
                api_key,
                project,
                environment,
                values: args.secrets,
                description: args.descriptions,
            };

            handle_set_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Description(args) => {
            let args = HandleDescriptionArgs {
                api_key,
                project,
                environment,
                description: args.description,
                key: args.key,
            };

            handle_update_description(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Upload(args) => {
            let args = HandleUploadSecretsArgs {
                api_key,
                project,
                environment,
                file_path: args.file_path,
            };

            handle_upload_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        SecretSubcommand::Rename(args) => {
            let args = HandleRenameSecretsArgs {
                api_key,
                project,
                environment,
                secrets: args.secrets,
            };

            handle_rename_secrets(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
    }
}
