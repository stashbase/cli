use crate::{
    cmd::environments::{
        EnvChangelogSubcommand, EnvironmentCommands, EnvironmentFormat, EnvironmentSubcommand,
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
};

pub async fn handle_environment_commands(
    cmd: EnvironmentCommands,
    api_key: String,
    raw_output: bool,
) {
    if let EnvironmentSubcommand::Changelog(c) = &cmd.subcommand {
        let (project, environment) = match cmd.try_get_project_environment() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        };

        match &c.subcommand {
            EnvChangelogSubcommand::List(args) => {
                let args = HandleEnvChangelogListArgs {
                    api_key,
                    project,
                    environment,
                    show_values: args.show_values,
                    page: args.page,
                    raw: raw_output,
                };

                handle_list_changelog(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
            EnvChangelogSubcommand::Revert(args) => {
                let args = HandleRevertEnvChangelogChange {
                    api_key,
                    project,
                    environment,
                    change_id: args.id.to_owned(),
                };

                handle_revert_changelog_change(args)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
            }
            EnvChangelogSubcommand::Get(args) => {
                let args = HandleGetEnvChangelogItemArgs {
                    api_key,
                    project,
                    environment,
                    raw: raw_output,
                    change_id: args.id.to_owned(),
                };

                handle_get_changelog_item(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
        }
    } else {
        let project = match cmd.try_get_project() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        };

        match cmd.subcommand {
            EnvironmentSubcommand::List(args) => {
                let args = HandleListEnvironmentsArgs {
                    api_key,
                    project,
                    search: args.search,
                    sort: args.sort,
                    descending: args.descending,
                    types: args.types,
                    locked: args.locked,
                    unlocked: args.unlocked,
                    format: match raw_output {
                        true => EnvironmentFormat::Json,
                        false => args.format.unwrap_or_default(),
                    },
                };

                handle_list_environments(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }

            EnvironmentSubcommand::Get(args) => {
                handle_get_environment(
                    api_key,
                    match raw_output {
                        true => EnvironmentFormat::Json,
                        false => args.format.unwrap_or_default(),
                    },
                    project,
                    args.name,
                )
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
            EnvironmentSubcommand::Open(args) => {
                handle_open_environment(api_key, project, args.name)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
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

                handle_create_environment(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }

            EnvironmentSubcommand::SetType(args) => {
                handle_update_env_type(api_key, project, args.name, args.env_type)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
            }
            EnvironmentSubcommand::Lock(args) => {
                handle_set_env_lock(api_key, project, args.name, true)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
            }
            EnvironmentSubcommand::Unlock(args) => {
                handle_set_env_lock(api_key, project, args.name, false)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
            }
            EnvironmentSubcommand::Delete(args) => {
                handle_delete_environment(api_key, project, args.name)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    })
            }
            EnvironmentSubcommand::Update(args) => handle_update_environment(
                api_key,
                project,
                args.name,
                args.new_name,
                args.description,
            )
            .await
            .unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            }),
            EnvironmentSubcommand::Duplicate(args) => {
                handle_duplicate_environment(api_key, project, args.name, args.new_name)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    })
            }

            EnvironmentSubcommand::Compare(args) => {
                let handler_args = HandleCompareEnvironmentsArgs {
                    api_key,
                    project,
                    environment_1: args.name_1,
                    environment_2: args.name_2,
                    only_keys: args.only_keys,
                    json_format: raw_output,
                };

                handle_compare_environments(handler_args)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
            }
            EnvironmentSubcommand::Changelog(_) => {
                unreachable!()
            }
        }
    }
}
