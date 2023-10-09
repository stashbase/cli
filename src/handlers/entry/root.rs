use log::debug;

use crate::{
    cmd::root::{Cli, EntityType},
    config::config,
    handlers::entry::{
        config::handle_config_commands, environments::handle_environment_commands,
        projects::handle_project_commands, secrets::handle_secrets_commands,
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
        }
    } else {
        let err = config.unwrap_err();
        println!("{:?}", err);
    }
}
