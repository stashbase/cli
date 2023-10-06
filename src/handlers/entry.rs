use log::debug;

use crate::{
    cmd::{
        configs::{ConfigSubcommand, SetConfigSubcommand},
        projects::ProjectSubcommand,
        root::{Cli, EntityType},
    },
    config::config,
    handlers::projects::{
        create::handle_create_project, delete::handle_delete_project, get::handle_get_project,
        list::handle_list_projects, open::handle_open_project,
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
