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
        run::entry::{handle_load_env_run, HandleRunArgs},
    },
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
            EntityType::Project(cmd) => {
                handle_project_commands(cmd, token, raw_output).await;
            }
            EntityType::Environment(cmd) => {
                handle_environment_commands(cmd, token, raw_output).await;
            }
            EntityType::Config(cmd) => {
                handle_config_commands(cmd).await;
            }
            EntityType::Secret(cmd) => {
                handle_secrets_commands(cmd, token, raw_output).await;
            }
            EntityType::Run(args) => {
                let args = HandleRunArgs {
                    token,
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
            EntityType::Open => {
                handle_open_dashboard(token).await.unwrap_or_else(|err| {
                    eprintln!("{:?}", err);
                });
            }
        }
    } else {
        let err = config.unwrap_err();
        eprintln!("{:?}", err);
    }
}
