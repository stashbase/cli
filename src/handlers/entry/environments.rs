use anyhow::{bail, Result};

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
    models::{
        scope::Scope,
        validation::{CmdArgInputValidationError, InputValidationError},
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
    fn try_get_project_with_spacing(cmd: &EnvironmentCommands, silent: bool) -> Result<String> {
        match cmd.try_get_project() {
            Ok(project) => Ok(project),
            Err(err) => {
                if !silent {
                    eprintln!();
                }
                bail!(err);
            }
        }
    }

    if let EnvironmentSubcommand::Get(get_cmd) = &cmd.subcommand {
        let format = get_output_format(
            raw_output,
            default_output_format.clone(),
            get_cmd.format.clone(),
        );

        if get_cmd.scope == Some(Scope::Environment) {
            handle_get_environment(api_key.clone(), format, silent, None, None).await?;
        } else {
            match get_cmd.identifier.clone() {
                Some(identifier) => {
                    let project = try_get_project_with_spacing(&cmd, silent)?;

                    handle_get_environment(
                        api_key.clone(),
                        format,
                        silent,
                        Some(project),
                        Some(identifier),
                    )
                    .await?;
                }
                None => {
                    try_get_project_with_spacing(&cmd, silent)?;

                    let error = InputValidationError::CmdArgs(
                        CmdArgInputValidationError::MissingEnvironmentIdentifierArgument,
                    );
                    let formatted_err = error.format_error_output(format == OutputFormat::Json)?;

                    if !silent {
                        eprintln!();
                    }

                    bail!(formatted_err);
                }
            }
        }

        return Ok(());
    } else if let EnvironmentSubcommand::Open(open_cmd) = &cmd.subcommand {
        if open_cmd.scope == Some(Scope::Environment) {
            handle_open_environment(api_key.clone(), None, None, raw_output, silent).await?;
        } else {
            match open_cmd.identifier.clone() {
                Some(identifier) => {
                    let project = try_get_project_with_spacing(&cmd, silent)?;

                    handle_open_environment(
                        api_key.clone(),
                        Some(project),
                        Some(identifier),
                        raw_output,
                        silent,
                    )
                    .await?;
                }
                None => {
                    try_get_project_with_spacing(&cmd, silent)?;

                    let error = InputValidationError::CmdArgs(
                        CmdArgInputValidationError::MissingEnvironmentIdentifierArgument,
                    );
                    let formatted_err = error.format_error_output(raw_output)?;

                    if !silent {
                        eprintln!();
                    }

                    bail!(formatted_err);
                }
            }
        }

        return Ok(());
    };

    let project = try_get_project_with_spacing(&cmd, silent)?;

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
                include_values: args.include_values,
                expand_refs: args.expand_refs.unwrap_or(false),
                json_format: raw_output,
                silent,
            };

            handle_compare_environments(handler_args).await?;
        }
        EnvironmentSubcommand::Get(_) => unreachable!(),
        EnvironmentSubcommand::Open(_) => unreachable!(),
    }

    Ok(())
}
