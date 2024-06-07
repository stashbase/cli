use anyhow::Result;

use crate::{
    cmd::{
        configs::OutputFormat,
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
};

pub async fn handle_project_commands(
    cmd: ProjectCommands,
    api_key: String,
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
) -> Result<()> {
    match cmd.subcommand {
        ProjectSubcommand::List(args) => {
            let args = HandleListProjectsArgs {
                api_key,
                search: args.search,
                sort: args.sort,
                descending: args.descending,
                format: match raw_output {
                    true => OutputFormat::Json,
                    false => args
                        .format
                        .unwrap_or(default_output_format.unwrap_or_default()),
                },
            };

            handle_list_projects(args).await?;
        }

        ProjectSubcommand::Get(args) => {
            handle_get_project(
                api_key,
                match raw_output {
                    true => OutputFormat::Json,
                    false => args
                        .format
                        .unwrap_or(default_output_format.unwrap_or_default()),
                },
                args.name,
            )
            .await?;
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
