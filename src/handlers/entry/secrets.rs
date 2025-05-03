use anyhow::{bail, Result};

use crate::{
    cmd::{
        config::SecretsOutputFormat,
        secrets::{SecretArgs, SecretSubcommand},
    },
    handlers::secrets::{
        comment::{handle_update_comment, HandleCommentArgs},
        create::{handle_create_secrets, HandleCreateSecretsArgs},
        delete::{handle_delete_secrets, HandleDeleteSecretsArgs},
        get::{handle_get_secrets, HandleGetSecretsArgs},
        list::{handle_list_secrets, HandleListSecretsArgs},
        rename::{handle_rename_secrets, HandleRenameSecretsArgs},
        search::{handle_search_secrets, HandleSearchSecretsArgs},
        set::{handle_set_secrets, HandleSetSecretsArgs},
        update::{handle_update_secrets, HandleUpdateSecretsArgs},
        upload::{handle_upload_secrets, HandleUploadSecretsArgs},
    },
    models::secrets::SecretsSearchOutputFormat,
    utils::validation::validate_project_environment_identifier,
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
    expand_refs: Option<bool>,
    default_output_format: Option<SecretsOutputFormat>,
) -> Result<()> {
    if let SecretSubcommand::Search(args) = cmd.subcommand {
        let format = match raw_output {
            true => SecretsSearchOutputFormat::Json,
            false => match args.format {
                Some(format) => format,
                None => match default_output_format {
                    Some(default_format) => {
                        let search_format: Option<SecretsSearchOutputFormat> =
                            default_format.into();

                        search_format.unwrap_or_default()
                    }
                    None => SecretsSearchOutputFormat::default(),
                },
            },
        };

        let args = HandleSearchSecretsArgs {
            api_key,
            format,
            project: args.project,
            name: args.name,
            value: args.value,
            show_values: args.show_values,
            with_ids: args.with_ids,
        };

        handle_search_secrets(args).await?;

        return Ok(());
    }

    let (project, environment) = cmd.try_get_project_environment()?;

    let validation_res = validate_project_environment_identifier(&project, &environment, false);

    if let Err(err) = validation_res {
        eprintln!();
        bail!(err);
    }

    match cmd.subcommand {
        SecretSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleListSecretsArgs {
                api_key,
                project,
                format,
                environment,
                only_names: args.only_names,
                expand_refs: args.expand_refs.unwrap_or(expand_refs.unwrap_or(false)),
            };

            handle_list_secrets(args).await?;
        }
        SecretSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleGetSecretsArgs {
                api_key,
                format,
                project,
                environment,
                names: args.names,
                expand_refs: args.expand_refs.unwrap_or(expand_refs.unwrap_or(false)),
            };

            handle_get_secrets(args).await?;
        }
        SecretSubcommand::Delete(args) => {
            let args = HandleDeleteSecretsArgs {
                api_key,
                project,
                environment,
                names: args.names,
                delete_all: args.delete_all,
                json_format: raw_output,
            };

            handle_delete_secrets(args).await?;
        }
        SecretSubcommand::Set(args) => {
            let args = HandleSetSecretsArgs {
                api_key,
                project,
                environment,
                values: args.secrets,
                comment: args.comments,
                json_format: raw_output,
            };

            handle_set_secrets(args).await?;
        }
        SecretSubcommand::Create(args) => {
            // let json_format = match raw_output {
            //     true => true,
            //     false => default_output_format.unwrap_or_default() == SecretsOutputFormat::Json,
            // };

            let args = HandleCreateSecretsArgs {
                api_key,
                project,
                environment,
                values: args.secrets,
                comments: args.comments,
                json_format: raw_output,
            };

            handle_create_secrets(args).await?;
        }
        SecretSubcommand::Update(args) => {
            let args = HandleUpdateSecretsArgs {
                api_key,
                project,
                environment,
                new_names: args.new_names,
                values: args.values,
                comment: args.comments,
                json_format: raw_output,
            };

            handle_update_secrets(args).await?;
        }
        SecretSubcommand::Upload(args) => {
            let args = HandleUploadSecretsArgs {
                api_key,
                project,
                environment,
                format: args.format,
                file_path: args.file_path,
                json_format: raw_output,
            };

            handle_upload_secrets(args).await?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
