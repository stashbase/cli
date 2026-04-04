use anyhow::{bail, Result};

use crate::{
    cmd::{
        config::SecretsOutputFormat,
        secrets::{SecretArgs, SecretSubcommand},
    },
    handlers::secrets::{
        create::{handle_create_secrets, HandleCreateSecretsArgs},
        delete::{handle_delete_secrets, HandleDeleteSecretsArgs},
        diff::{handle_secrets_diff, HandleSecretsDiffArgs},
        get::{handle_get_secrets, HandleGetSecretsArgs},
        list::{handle_list_secrets, HandleListSecretsArgs},
        search::{handle_search_secrets, HandleSearchSecretsArgs},
        set::{handle_set_secrets, HandleSetSecretsArgs},
        update::{handle_update_secrets, HandleUpdateSecretsArgs},
        upload::{handle_upload_secrets, HandleUploadSecretsArgs},
    },
    models::{
        scope::Scope,
        secrets::SecretsSearchOutputFormat,
        validation::{CmdArgInputValidationError, InputValidationError},
    },
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
    silent: bool,
    default_output_format: Option<SecretsOutputFormat>,
) -> Result<()> {
    if let SecretSubcommand::Search(args) = cmd.subcommand {
        // Check if scope is provided for commands that don't support it
        if cmd.scope.is_some() || args.scope.is_some() {
            let error = InputValidationError::CmdArgs(
                CmdArgInputValidationError::ScopeNotSupportedForCommand,
            );

            let formatted_err = error.format_error_output(raw_output)?;

            if !silent {
                eprintln!();
            }

            bail!(formatted_err);
        }

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
            silent,
            project: args.project,
            name: args.name,
            value: args.value,
            return_values: args.return_values,
        };

        handle_search_secrets(args).await?;

        return Ok(());
    }

    let scope = match cmd.get_scope() {
        Ok(s) => s,
        Err(e) => {
            if !silent {
                eprintln!();
            }

            bail!(e.format_error_output(raw_output)?);
        }
    };

    let is_environment_scope = scope == Scope::Environment;

    let mut project: Option<String> = None;
    let mut environment: Option<String> = None;

    if !is_environment_scope {
        let project_env_res = cmd.try_get_project_environment();

        if let Err(err) = project_env_res {
            let formatted_err = err.format_error_output(raw_output)?;

            if !silent {
                eprintln!();
            }
            bail!(formatted_err);
        }

        let (p, env) = project_env_res.unwrap();

        let validation_res =
            validate_project_environment_identifier(p.as_ref(), env.as_ref(), false);

        if let Err(err) = validation_res {
            let formatted_err = err.format_error_output(raw_output)?;

            if !silent {
                eprintln!();
            }
            bail!(formatted_err);
        }

        project = Some(p);
        environment = Some(env);
    }

    match cmd.subcommand {
        SecretSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleListSecretsArgs {
                api_key,
                project,
                format,
                silent,
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
                silent,
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
                silent,
                project,
                environment,
                force: args.force,
                names: args.names,
                delete_all: args.delete_all,
                json_format: raw_output,
            };

            handle_delete_secrets(args).await?;
        }
        SecretSubcommand::Set(args) => {
            let args = HandleSetSecretsArgs {
                api_key,
                silent,
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
                silent,
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
                silent,
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
                silent,
                project,
                environment,
                format: args.format,
                file_path: args.file_path,
                json_format: raw_output,
                ignore_comments: args.ignore_comments.unwrap_or(false),
            };

            handle_upload_secrets(args).await?;
        }
        SecretSubcommand::Diff(args) => {
            // Check if scope is provided for commands that don't support it
            if cmd.scope.is_some() || args.scope.is_some() {
                let error = InputValidationError::CmdArgs(
                    CmdArgInputValidationError::ScopeNotSupportedForCommand,
                );

                let formatted_err = error.format_error_output(raw_output)?;

                if !silent {
                    eprintln!();
                }

                bail!(formatted_err);
            }

            let args = HandleSecretsDiffArgs {
                api_key,
                silent,
                project,
                environment,
                file_path: args.file_path,
                format: args.format,
                json_format: raw_output,
                with_comments: args.with_comments,
                show_values: args.show_values,
                expand_refs: args.expand_refs.unwrap_or(expand_refs.unwrap_or(false)),
            };

            handle_secrets_diff(args).await?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
