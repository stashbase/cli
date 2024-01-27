use log::debug;

use crate::{
    cmd::root::{Cli, EntityType},
    config::config,
    handlers::{
        entry::{
            config::handle_config_commands, environments::handle_environment_commands,
            projects::handle_project_commands, secrets::handle_secrets_commands,
        },
        open::handle_open_dashboard,
        pull::entry::{handle_pull, HandlePullArgs},
        run::entry::{handle_load_env_run, HandleRunArgs},
    },
};

#[tokio::main()]
pub async fn handle_cli(args: Cli) {
    debug!("args: {:?}", args);

    let config = config::get_config();
    debug!("config: {:?}", config);

    if let Ok(config) = config {
        if let EntityType::Config(cmd) = args.entity_type {
            handle_config_commands(cmd).await;
            return;
        }

        // TODO: check api_key for api commands
        let api_key = config.api_key.unwrap();

        let raw_output = args.raw;

        match args.entity_type {
            EntityType::Project(cmd) => {
                handle_project_commands(cmd, api_key, raw_output).await;
            }
            EntityType::Environment(cmd) => {
                handle_environment_commands(cmd, api_key, raw_output).await;
            }
            EntityType::Config(cmd) => {
                handle_config_commands(cmd).await;
            }
            EntityType::Secret(cmd) => {
                handle_secrets_commands(cmd, api_key, raw_output).await;
            }
            EntityType::Run(args) => {
                let args = HandleRunArgs {
                    api_key,
                    project: args.project,
                    environment: args.environment,
                    command: args.command,
                    exclude: args.exclude,
                    only: args.only,
                    set: args.set,
                    print_secrets: args.print_secrets,
                    file: args.file,
                };

                handle_load_env_run(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
            EntityType::Pull(args) => {
                let args = HandlePullArgs {
                    api_key,
                    file: args.config_file,
                    set: args.set,
                    output_file: args.output_file,
                    format: args.format,
                    only: args.only,
                    exclude: args.exclude,
                    print_secrets: args.print_secrets,
                };

                handle_pull(args).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
            EntityType::Open => {
                handle_open_dashboard(api_key).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
        }
    } else {
        let err = config.unwrap_err();
        eprintln!("{:?}", err);
    }
}
