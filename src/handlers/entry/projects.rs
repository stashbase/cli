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
                sort: args.sort,
                descending: args.descending,
                format,
            };

            handle_list_projects(args).await?;
        }

        ProjectSubcommand::Get(args) => {
            let format = get_output_format(raw_output, default_output_format, args.format);
            handle_get_project(api_key, format, args.name).await?;
        }

        ProjectSubcommand::Create(args) => {
            handle_create_project(api_key, args.name, args.description).await?;
        }
        ProjectSubcommand::Delete(args) => {
            handle_delete_project(api_key, args.name).await?;
        }
        ProjectSubcommand::Open(args) => {
            handle_open_project(api_key, args.name).await?;
        }
        ProjectSubcommand::Update(args) => {
            handle_update_project(api_key, args.name, args.new_name, args.description).await?;
        }
    }

    Ok(())
}
