use log::debug;

use crate::{
    cmd::{
        configs::{ConfigSubcommand, SetConfigSubcommand},
        environments::EnvironmentSubcommand,
        projects::ProjectSubcommand,
        root::{Cli, EntityType},
    },
    config::config,
    handlers::{
        environments::{
            create::handle_create_environment, get::handle_get_environment,
            list::handle_list_environments, open::handle_open_environment,
        },
        projects::{
            create::handle_create_project, delete::handle_delete_project, get::handle_get_project,
            list::handle_list_projects, open::handle_open_project, update::handle_update_project,
        },
    },
    models::config::UpdateConfig,
};

#[tokio::main()]
pub async fn handle_cli(args: Cli) {
    debug!("args: {:?}", args);

    let config = config::get_config();
    debug!("config: {:?}", config);

    if let Ok(config) = config {
        // TODO: check token for api commands
        let token = config.token.unwrap();

        let raw_output = args.raw;

        match args.entity_type {
            EntityType::Project(cmd) => match cmd.subcommand {
                ProjectSubcommand::List(_) => {
                    handle_list_projects(token, raw_output)
                        .await
                        .unwrap_or_else(|err| {
                            println!("{:?}", err);
                        });
                }

                ProjectSubcommand::Get(args) => {
                    handle_get_project(token, raw_output, args.name)
                        .await
                        .unwrap_or_else(|err| {
                            eprintln!("{:?}", err);
                        });
                }

                ProjectSubcommand::Create(args) => {
                    handle_create_project(token, args.name, args.description)
                        .await
                        .unwrap_or_else(|err| {
                            eprintln!("{:?}", err);
                        });
                }

                ProjectSubcommand::Delete(args) => {
                    handle_delete_project(token, args.name)
                        .await
                        .unwrap_or_else(|err| {
                            eprintln!("{:?}", err);
                        });
                }
                ProjectSubcommand::Open(args) => {
                    handle_open_project(token, args.name)
                        .await
                        .unwrap_or_else(|err| {
                            eprintln!("{:?}", err);
                        });
                }

                ProjectSubcommand::Update(args) => {
                    handle_update_project(token, args.name, args.new_name, args.description)
                        .await
                        .unwrap_or_else(|err| {
                            eprintln!("{:?}", err);
                        });
                }
            },

            EntityType::Environment(cmd) => match cmd.subcommand {
                EnvironmentSubcommand::List(args) => {
                    handle_list_environments(token, raw_output, cmd.project)
                        .await
                        .unwrap_or_else(|err| {
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
                    handle_create_environment(
                        token,
                        cmd.project,
                        args.name,
                        args.env_type,
                        args.description,
                        args.open,
                    )
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
                }
            },
            EntityType::Config(cmd) => match cmd.subcommand {
                ConfigSubcommand::Set(args) => match args.subcommand {
                    SetConfigSubcommand::Token(t) => {
                        config::update_config(UpdateConfig {
                            token: Some(t.value),
                        })
                        .unwrap();
                    }
                },
            },
        }
    } else {
        let err = config.unwrap_err();
        println!("{:?}", err);
    }
}
