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
    match cmd.subcommand {
        EnvironmentSubcommand::List(args) => {
            let args = HandleListEnvironmentsArgs {
                api_key,
                project: cmd.project,
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
                cmd.project,
                args.name,
            )
            .await
            .unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }
        EnvironmentSubcommand::Open(args) => {
            handle_open_environment(api_key, cmd.project, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        EnvironmentSubcommand::Create(args) => {
            let args = HandleCreateEnvironmentArgs {
                api_key,
                project: cmd.project,
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
            handle_update_env_type(api_key, cmd.project, args.name, args.env_type)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Lock(args) => {
            handle_set_env_lock(api_key, cmd.project, args.name, true)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Unlock(args) => {
            handle_set_env_lock(api_key, cmd.project, args.name, false)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Delete(args) => {
            handle_delete_environment(api_key, cmd.project, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                })
        }
        EnvironmentSubcommand::Update(args) => handle_update_environment(
            api_key,
            cmd.project,
            args.name,
            args.new_name,
            args.description,
        )
        .await
        .unwrap_or_else(|err| {
            eprintln!("{:?}", err);
        }),
        EnvironmentSubcommand::Duplicate(args) => {
            handle_duplicate_environment(api_key, cmd.project, args.name, args.new_name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                })
        }
        EnvironmentSubcommand::Changelog(changelog_args) => match changelog_args.subcommand {
            EnvChangelogSubcommand::List(args) => {
                let args = HandleEnvChangelogListArgs {
                    api_key,
                    project: cmd.project,
                    environment: changelog_args.environment,
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
                    project: cmd.project,
                    environment: changelog_args.environment,
                    change_id: args.id,
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
                    project: cmd.project,
                    environment: changelog_args.environment,
                    change_id: args.id,
                    raw: raw_output,
                };

                handle_get_changelog_item(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
        },
    }
}
