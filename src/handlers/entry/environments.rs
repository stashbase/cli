use anyhow::Result;

use crate::{
    cmd::{
        config::OutputFormat,
        environments::{EnvironmentCommands, EnvironmentSubcommand},
    },
    handlers::environments::{
        compare::{handle_compare_environments, HandleCompareEnvironmentsArgs},
        create::{handle_create_environment, HandleCreateEnvironmentArgs},
        delete::{handle_delete_environment, HandleDeleteEnvironmentArgs},
        get::handle_get_environment,
        list::{handle_list_environments, HandleListEnvironmentsArgs},
        open::handle_open_environment,
        update::{handle_update_environment, HandleUpdateEnvironmentArgs},
    },
    utils::output::get_output_format,
};

pub async fn handle_environment_commands(
    cmd: EnvironmentCommands,
    api_key: String,
    raw_output: bool,
    silent: bool,
    default_output_format: Option<OutputFormat>,
) -> Result<()> {
    let project = cmd.try_get_project()?;

    match cmd.subcommand {
        EnvironmentSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleListEnvironmentsArgs {
                api_key,
                project,
                silent,
                search: args.search,
                sort_by: args.sort_by,
                descending: args.descending,
                is_production: args.is_production,
                format,
            };

            handle_list_environments(args).await?;
        }

        EnvironmentSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);
            handle_get_environment(api_key, format, silent, project, args.identifier).await?;
        }
        EnvironmentSubcommand::Open(args) => {
            handle_open_environment(api_key, project, args.identifier, raw_output, silent).await?;
        }
        EnvironmentSubcommand::Create(args) => {
            let args = HandleCreateEnvironmentArgs {
                api_key,
                project,
                name: args.name,
                description: args.description,
                is_production: args.is_production,
                open: args.open,
                format: args.file_format,
                file_path: args.file_path,
                json_format: raw_output,
                force: args.force,
                silent,
            };

            handle_create_environment(args).await?;
        }

        EnvironmentSubcommand::Delete(args) => {
            let args = HandleDeleteEnvironmentArgs {
                api_key,
                project,
                environment: args.identifier,
                json_format: raw_output,
                silent,
                force: args.force,
            };

            handle_delete_environment(args).await?;
        }

        EnvironmentSubcommand::Update(args) => {
            let args = HandleUpdateEnvironmentArgs {
                api_key,
                project,
                environment: args.identifier,
                new_name: args.new_name,
                new_description: args.description,
                new_is_production: args.is_production,
                json_format: raw_output,
                force: args.force,
                silent,
            };

            handle_update_environment(args).await?;
        }
        EnvironmentSubcommand::Compare(args) => {
            let handler_args = HandleCompareEnvironmentsArgs {
                api_key,
                project,
                environment_1: args.identifier_1,
                environment_2: args.identifier_2,
                only_names: args.only_names,
                json_format: raw_output,
                silent,
            };

            handle_compare_environments(handler_args).await?;
        }
    }

    Ok(())
}
