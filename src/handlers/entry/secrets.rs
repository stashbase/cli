use anyhow::Result;

use crate::{
    cmd::{
        config::SecretsOutputFormat,
        secrets::{SecretArgs, SecretSubcommand},
    },
    handlers::secrets::{
        delete::{handle_delete_secrets, HandleDeleteSecretsArgs},
        description::{handle_update_description, HandleDescriptionArgs},
        get::{handle_get_secrets, HandleGetSecretsArgs},
        list::{handle_list_secrets, HandleListSecretsArgs},
        rename::{handle_rename_secrets, HandleRenameSecretsArgs},
        set::{handle_set_secrets, HandleSetSecretsArgs},
        upload::{handle_upload_secrets, HandleUploadSecretsArgs},
    },
    models::config::State,
};

fn get_output_format(
    raw_output: bool,
    default_output_format: Option<SecretsOutputFormat>,
    cmd_format: Option<SecretsOutputFormat>,
) -> SecretsOutputFormat {
    match raw_output {
        true => SecretsOutputFormat::Json,
        false => cmd_format.unwrap_or(default_output_format.unwrap_or_default()),
    }
}

pub async fn handle_secrets_commands(
    cmd: SecretArgs,
    api_key: String,
    raw_output: bool,
    default_output_format: Option<SecretsOutputFormat>,
    state: &Option<State>,
) -> Result<()> {
    let (state_project, state_env) = match state {
        Some(s) => {
            eprint!("{}", s);
            (&s.project, &s.environment)
        }
        None => (&None, &None),
    };

    let (project, environment) = cmd.try_get_project_environment(state_project, state_env)?;

    match cmd.subcommand {
        SecretSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleListSecretsArgs {
                api_key,
                project,
                environment,
                only_keys: args.only_keys,
                format,
            };

            handle_list_secrets(args).await?;
        }
        SecretSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleGetSecretsArgs {
                api_key,
                project,
                environment,
                keys: args.keys,
                format,
            };

            handle_get_secrets(args).await?;
        }
        SecretSubcommand::Delete(args) => {
            let args = HandleDeleteSecretsArgs {
                api_key,
                project,
                environment,
                keys: args.keys,
                delete_all: args.delete_all,
            };

            handle_delete_secrets(args).await?;
        }
        SecretSubcommand::Set(args) => {
            let args = HandleSetSecretsArgs {
                api_key,
                project,
                environment,
                values: args.secrets,
                description: args.descriptions,
            };

            handle_set_secrets(args).await?;
        }
        SecretSubcommand::Description(args) => {
            let args = HandleDescriptionArgs {
                api_key,
                project,
                environment,
                description: args.description,
                key: args.key,
            };

            handle_update_description(args).await?;
        }
        SecretSubcommand::Upload(args) => {
            let args = HandleUploadSecretsArgs {
                api_key,
                project,
                environment,
                file_path: args.file_path,
            };

            handle_upload_secrets(args).await?;
        }
        SecretSubcommand::Rename(args) => {
            let args = HandleRenameSecretsArgs {
                api_key,
                project,
                environment,
                secrets: args.secrets,
            };

            handle_rename_secrets(args).await?;
        }
    }

    Ok(())
}
