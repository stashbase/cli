use anyhow::{bail, Result};

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
    models::{
        config::State,
        validation::{InputValidationError, ProjectInputValidationError},
    },
    utils::output::get_output_format,
};

pub async fn handle_project_commands(
    cmd: ProjectCommands,
    api_key: String,
    raw_output: bool,
    default_output_format: Option<OutputFormat>,
    state: &Option<State>,
) -> Result<()> {
    if let ProjectSubcommand::List(args) = cmd.subcommand {
        let format = get_output_format(raw_output, default_output_format, args.format);

        let args = HandleListProjectsArgs {
            api_key,
            search: args.search,
            sort: args.sort,
            descending: args.descending,
            format,
        };

        handle_list_projects(args).await?;
    } else if let ProjectSubcommand::Create(args) = cmd.subcommand {
        handle_create_project(api_key, args.name, args.description).await?;
    } else {
        if let Some(s) = state {
            eprint!("{s}");
        }

        let name_argument = cmd.get_name_argument();
        let project = get_project_name(state, name_argument)?;

        match cmd.subcommand {
            ProjectSubcommand::Get(args) => {
                let format = get_output_format(raw_output, default_output_format, args.format);
                handle_get_project(api_key, format, project).await?;
            }

            ProjectSubcommand::Delete(_) => {
                handle_delete_project(api_key, project).await?;
            }
            ProjectSubcommand::Open(_) => {
                handle_open_project(api_key, project).await?;
            }
            ProjectSubcommand::Update(args) => {
                handle_update_project(api_key, project, args.new_name, args.description).await?;
            }
            _ => unreachable!(),
        }
    }

    Ok(())
}

fn get_project_name(state: &Option<State>, name_arg: Option<String>) -> Result<String> {
    if let Some(s) = state {
        if let Some(project) = &s.project {
            match name_arg {
                Some(arg_name) => Ok(arg_name),
                None => Ok(project.to_string()),
            }
        } else {
            match name_arg {
                Some(arg_name) => Ok(arg_name),
                None => {
                    let err = InputValidationError::Projects(
                        ProjectInputValidationError::ProjectStateNotSet,
                    );
                    bail!(err);
                }
            }
        }
    } else {
        match name_arg {
            Some(arg_name) => Ok(arg_name),
            None => {
                let err =
                    InputValidationError::Projects(ProjectInputValidationError::NameNotProvided);
                bail!(err);
            }
        }
    }
}
