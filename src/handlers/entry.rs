use log::debug;

use crate::{
    cmd::{
        configs::{ConfigSubcommand, SetConfigSubcommand},
        environments::EnvironmentSubcommand,
        projects::ProjectSubcommand,
        root::{Cli, EntityType},
        secrets::{SecretSubcommand, SecretsFromat},
    },
    config::config,
    handlers::{
        environments::{
            create::handle_create_environment, delete::handle_delete_environment,
            get::handle_get_environment, list::handle_list_environments,
            open::handle_open_environment, set_lock::handle_set_env_lock,
            update::handle_update_environment, update_type::handle_update_env_type,
        },
        projects::{
            create::handle_create_project, delete::handle_delete_project, get::handle_get_project,
            list::handle_list_projects, open::handle_open_project, update::handle_update_project,
        },
        secrets::{
            delete::{handle_delete_secrets, HandleDeleteSecretsArgs},
            get::{handle_get_secrets, HandleGetSecretsArgs},
            list::{handle_list_secrets, HandleListSecretsArgs},
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
            EntityType::Secret(cmd) => match cmd.subcommand {
                SecretSubcommand::List(args) => {
                    let args = HandleListSecretsArgs {
                        token,
                        only_keys: args.only_keys,
                        project: cmd.project,
                        environment: cmd.environment,
                        format: args.format.unwrap_or(if raw_output {
                            SecretsFromat::Json
                        } else {
                            SecretsFromat::List
                        }),
                    };

                    handle_list_secrets(args).await.unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
                }
                SecretSubcommand::Get(args) => {
                    let args = HandleGetSecretsArgs {
                        token,
                        project: cmd.project,
                        environment: cmd.environment,
                        keys: args.keys,
                        format: args.format.unwrap_or(if raw_output {
                            SecretsFromat::Json
                        } else {
                            SecretsFromat::List
                        }),
                    };

                    handle_get_secrets(args).await.unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
                }
                SecretSubcommand::Delete(args) => {
                    let args = HandleDeleteSecretsArgs {
                        token,
                        project: cmd.project,
                        environment: cmd.environment,
                        keys: args.keys,
                        delete_all: args.delete_all,
                    };

                    handle_delete_secrets(args).await.unwrap_or_else(|err| {
                        eprintln!("{:?}", err);
                    });
                }
            },
        }
    } else {
        let err = config.unwrap_err();
        println!("{:?}", err);
    }
}
