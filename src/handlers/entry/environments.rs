use crate::{
    cmd::environments::{EnvironmentCommands, EnvironmentSubcommand},
    handlers::environments::{
        create::{handle_create_environment, HandleCreateEnvironmentArgs},
        delete::handle_delete_environment,
        get::handle_get_environment,
        list::{handle_list_environments, HandleListEnvironmentsArgs},
        open::handle_open_environment,
        set_lock::handle_set_env_lock,
        update::handle_update_environment,
        update_type::handle_update_env_type,
    },
};

pub async fn handle_environment_commands(
    cmd: EnvironmentCommands,
    token: String,
    raw_output: bool,
) {
    match cmd.subcommand {
        EnvironmentSubcommand::List(args) => {
            let args = HandleListEnvironmentsArgs {
                token,
                project: cmd.project,
                sort: args.sort,
                descending: args.descending,
                raw: raw_output,
            };

            handle_list_environments(args).await.unwrap_or_else(|err| {
                eprintln!("{:?}", err);
            });
        }

        EnvironmentSubcommand::Get(args) => {
            handle_get_environment(token, raw_output, cmd.project, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Open(args) => {
            handle_open_environment(token, cmd.project, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }

        EnvironmentSubcommand::Create(args) => {
            let args = HandleCreateEnvironmentArgs {
                token,
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
            handle_update_env_type(token, cmd.project, args.name, args.env_type)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Lock(args) => {
            handle_set_env_lock(token, cmd.project, args.name, true)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Unlock(args) => {
            handle_set_env_lock(token, cmd.project, args.name, false)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
        }
        EnvironmentSubcommand::Delete(args) => {
            handle_delete_environment(token, cmd.project, args.name)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                })
        }
        EnvironmentSubcommand::Update(args) => handle_update_environment(
            token,
            cmd.project,
            args.name,
            args.new_name,
            args.description,
        )
        .await
        .unwrap_or_else(|err| {
            eprintln!("{:?}", err);
        }),
    }
}
