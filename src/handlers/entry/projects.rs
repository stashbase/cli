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
            let project = get_project_name(state, args.name)?;
            handle_get_project(api_key, format, project).await?;
        }

        ProjectSubcommand::Create(args) => {
            handle_create_project(api_key, args.name, args.description).await?;
        }
        ProjectSubcommand::Delete(args) => {
            let project = get_project_name(state, args.name)?;
            handle_delete_project(api_key, project).await?;
        }
        ProjectSubcommand::Open(args) => {
            let project = get_project_name(state, args.name)?;
            handle_open_project(api_key, project).await?;
        }
        ProjectSubcommand::Update(args) => {
            let project = get_project_name(state, args.name)?;
            handle_update_project(api_key, project, args.new_name, args.description).await?;
        }
    }

    Ok(())
}

fn get_project_name(state: &Option<State>, arg_name: Option<String>) -> Result<String> {
    if let Some(s) = state {
        eprint!("{s}");

        if let Some(project) = &s.project {
            match arg_name {
                Some(arg_name) => Ok(arg_name),
                None => Ok(project.to_string()),
            }
        } else {
            match arg_name {
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
        match arg_name {
            Some(arg_name) => Ok(arg_name),
            None => {
                let err =
                    InputValidationError::Projects(ProjectInputValidationError::NameNotProvided);
                bail!(err);
            }
        }
    }
}
