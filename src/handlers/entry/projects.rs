use std::os::linux::raw;

use anyhow::Result;

use crate::{
    cmd::{
        config::OutputFormat,
        projects::{ProjectCommands, ProjectSubcommand},
    },
    handlers::projects::{
        create::handle_create_project,
        delete::handle_delete_project,
        get::handle_get_project,
        list::{handle_list_projects, HandleListProjectsArgs},
        open::handle_open_project,
        update::handle_update_project,
    },
    utils::output::get_output_format,
};

pub async fn handle_project_commands(
    cmd: ProjectCommands,
    api_key: String,
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
) -> Result<()> {
    match cmd.subcommand {
        ProjectSubcommand::List(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);

            let args = HandleListProjectsArgs {
                api_key,
                search: args.search,
                sort_by: args.sort_by,
                descending: args.descending,
                page: args.page,
                limit: args.limit,
                format,
            };

            handle_list_projects(args).await?;
        }

        ProjectSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);
            handle_get_project(api_key, format, args.identifier).await?;
        }

        ProjectSubcommand::Create(args) => {
            handle_create_project(api_key, args.name, args.description, raw_output).await?;
        }
        ProjectSubcommand::Delete(args) => {
            handle_delete_project(api_key, args.identifier, raw_output).await?;
        }
        ProjectSubcommand::Open(args) => {
            handle_open_project(api_key, args.identifier, raw_output).await?;
        }
        ProjectSubcommand::Update(args) => {
            handle_update_project(
                api_key,
                args.identifier,
                args.new_name,
                args.description,
                raw_output,
            )
            .await?;
        }
    }

    Ok(())
}
