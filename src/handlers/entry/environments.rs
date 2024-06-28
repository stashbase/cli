use anyhow::{bail, Result};

use crate::{
    cmd::{
        config::OutputFormat,
        environments::{EnvChangelogSubcommand, EnvironmentCommands, EnvironmentSubcommand},
    },
    handlers::{
        env_changelog::{
            get::{handle_get_changelog_item, HandleGetEnvChangelogItemArgs},
            list::{handle_list_changelog, HandleEnvChangelogListArgs},
            revert::{handle_revert_changelog_change, HandleRevertEnvChangelogChange},
        },
        environments::{
            compare::{handle_compare_environments, HandleCompareEnvironmentsArgs},
            create::{handle_create_environment, HandleCreateEnvironmentArgs},
            delete::handle_delete_environment,
            duplicate::handle_duplicate_environment,
            get::handle_get_environment,
            list::{handle_list_environments, HandleListEnvironmentsArgs},
            open::handle_open_environment,
            set_lock::handle_set_env_lock,
            update::handle_update_environment,
            update_type::handle_update_env_type,
        },
    },
    models::{
        config::State,
        validation::{EnvironmentsInputValidationError, InputValidationError},
    },
    utils::output::get_output_format,
};

pub fn is_json_output(raw_output: bool, default_output_format: Option<OutputFormat>) -> bool {
    match raw_output {
        true => true,
        false => match default_output_format == Some(OutputFormat::Json) {
            true => true,
            false => false,
        },
    }
}

pub async fn handle_environment_commands(
    cmd: EnvironmentCommands,
    api_key: String,
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
    state: &Option<State>,
) -> Result<()> {
    if let EnvironmentSubcommand::Changelog(c) = &cmd.subcommand {
        let (state_project, state_env) = match state {
            Some(s) => {
                eprint!("{}", s);
                (&s.project, &s.environment)
            }
            None => (&None, &None),
        };

        let (project, environment) = cmd.try_get_project_environment(state_project, state_env)?;

        match &c.subcommand {
            EnvChangelogSubcommand::List(args) => {
                let json_output = is_json_output(raw_output, default_output_format);

                let args = HandleEnvChangelogListArgs {
                    api_key,
                    project,
                    environment,
                    show_values: args.show_values,
                    page: args.page,
                    raw: json_output,
                };

                handle_list_changelog(args).await?;
            }
            EnvChangelogSubcommand::Revert(args) => {
                let args = HandleRevertEnvChangelogChange {
                    api_key,
                    project,
                    environment,
                    change_id: args.id.to_owned(),
                };

                handle_revert_changelog_change(args).await?;
            }
            EnvChangelogSubcommand::Get(args) => {
                let json_output = is_json_output(raw_output, default_output_format);

                let args = HandleGetEnvChangelogItemArgs {
                    api_key,
                    project,
                    environment,
                    raw: json_output,
                    change_id: args.id.to_owned(),
                };

                handle_get_changelog_item(args).await?;
            }
        }

        Ok(())
    } else {
        let state_project = match state {
            Some(s) => {
                eprint!("{s}");
                &s.project
            }
            None => &None,
        };
        let project = cmd.try_get_project(state_project)?;

        if let EnvironmentSubcommand::List(args) = &cmd.subcommand {
            let format =
                get_output_format(raw_output, default_output_format, args.format.to_owned());

            let args = HandleListEnvironmentsArgs {
                api_key,
                project,
                search: args.search.to_owned(),
                sort: args.sort.to_owned(),
                descending: args.descending,
                types: args.types.to_owned(),
                locked: args.locked,
                unlocked: args.unlocked,
                format,
            };

            handle_list_environments(args).await?;
        } else if let EnvironmentSubcommand::Compare(args) = &cmd.subcommand {
            let json_format = is_json_output(raw_output, default_output_format);

            let handler_args = HandleCompareEnvironmentsArgs {
                api_key,
                project,
                environment_1: args.name_1.to_owned(),
                environment_2: args.name_2.to_owned(),
                only_keys: args.only_keys,
                json_format,
            };

            handle_compare_environments(handler_args).await?;
        } else {
            let name_argument = cmd.get_name_argument();
            let env_name = get_env_name(state, name_argument)?;

            match cmd.subcommand {
                EnvironmentSubcommand::Get(args) => {
                    let format = get_output_format(raw_output, default_output_format, args.format);
                    handle_get_environment(api_key, format, project, env_name).await?;
                }
                EnvironmentSubcommand::Open(_) => {
                    handle_open_environment(api_key, project, env_name).await?;
                }
                EnvironmentSubcommand::Create(args) => {
                    let args = HandleCreateEnvironmentArgs {
                        api_key,
                        project,
                        name: args.name,
                        description: args.description,
                        env_type: args.env_type,
                        open: args.open,
                        file_path: args.file_path,
                    };

                    handle_create_environment(args).await?;
                }

                EnvironmentSubcommand::SetType(args) => {
                    handle_update_env_type(api_key, project, env_name, args.env_type).await?;
                }
                EnvironmentSubcommand::Lock(_) => {
                    handle_set_env_lock(api_key, project, env_name, true).await?;
                }
                EnvironmentSubcommand::Unlock(_) => {
                    handle_set_env_lock(api_key, project, env_name, false).await?;
                }
                EnvironmentSubcommand::Delete(_) => {
                    handle_delete_environment(api_key, project, env_name).await?;
                }
                EnvironmentSubcommand::Update(args) => {
                    handle_update_environment(
                        api_key,
                        project,
                        env_name,
                        args.new_name,
                        args.description,
                    )
                    .await?
                }
                EnvironmentSubcommand::Duplicate(args) => {
                    let env_name = get_env_name(state, args.name)?;
                    handle_duplicate_environment(api_key, project, env_name, args.new_name).await?;
                }
                _ => unreachable!(),
            }
        }

        Ok(())
    }
}

fn get_env_name(state: &Option<State>, name_arg: Option<String>) -> Result<String> {
    if let Some(s) = state {
        if let Some(env) = &s.environment {
            match name_arg {
                Some(arg_name) => Ok(arg_name),
                None => Ok(env.to_string()),
            }
        } else {
            match name_arg {
                Some(arg_name) => Ok(arg_name),
                None => {
                    let err = InputValidationError::Environments(
                        EnvironmentsInputValidationError::EnvironmentStateNotSet,
                    );
                    bail!(err);
                }
            }
        }
    } else {
        match name_arg {
            Some(arg_name) => Ok(arg_name),
            None => {
                let err = InputValidationError::Environments(
                    EnvironmentsInputValidationError::NameNotProvided,
                );
                bail!(err);
            }
        }
    }
}
