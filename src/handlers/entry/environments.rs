use anyhow::Result;

use crate::{
    cmd::{
        config::OutputFormat,
        environments::{EnvironmentCommands, EnvironmentSubcommand},
    },
    handlers::environments::{
        compare::{handle_compare_environments, HandleCompareEnvironmentsArgs},
        create::{handle_create_environment, HandleCreateEnvironmentArgs},
        delete::handle_delete_environment,
        get::handle_get_environment,
        list::{handle_list_environments, HandleListEnvironmentsArgs},
        open::handle_open_environment,
        set_lock::handle_set_env_lock,
        update::handle_update_environment,
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
) -> Result<()> {
    let project = cmd.try_get_project()?;

    match cmd.subcommand {
        EnvironmentSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleListEnvironmentsArgs {
                api_key,
                project,
                search: args.search,
                sort_by: args.sort_by,
                descending: args.descending,
                types: args.types,
                locked: args.locked,
                unlocked: args.unlocked,
                format,
            };

            handle_list_environments(args).await?;
        }

        EnvironmentSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);
            handle_get_environment(api_key, format, project, args.identifier).await?;
        }
        EnvironmentSubcommand::Open(args) => {
            handle_open_environment(api_key, project, args.identifier).await?;
        }
        EnvironmentSubcommand::Create(args) => {
            let args = HandleCreateEnvironmentArgs {
                api_key,
                project,
                name: args.name,
                description: args.description,
                env_type: args.env_type,
                open: args.open,
                format: args.file_format,
                file_path: args.file_path,
            };

            handle_create_environment(args).await?;
        }

        EnvironmentSubcommand::Lock(args) => {
            handle_set_env_lock(api_key, project, args.identifier, true).await?;
        }
        EnvironmentSubcommand::Unlock(args) => {
            handle_set_env_lock(api_key, project, args.identifier, false).await?;
        }
        EnvironmentSubcommand::Delete(args) => {
            handle_delete_environment(api_key, project, args.identifier).await?;
        }
        EnvironmentSubcommand::Update(args) => {
            handle_update_environment(
                api_key,
                project,
                args.identifier,
                args.new_name,
                args.description,
                args.env_type,
            )
            .await?
        }
        EnvironmentSubcommand::Compare(args) => {
            let json_format = is_json_output(raw_output, default_output_format);

            let handler_args = HandleCompareEnvironmentsArgs {
                api_key,
                project,
                environment_1: args.identifier_1,
                environment_2: args.identifier_2,
                only_names: args.only_names,
                json_format,
            };

            handle_compare_environments(handler_args).await?;
        }
    }

    Ok(())
}
