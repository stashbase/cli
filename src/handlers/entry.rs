use log::debug;

use crate::{
    cmd::{
        configs::{ConfigSubcommand, SetConfigSubcommand},
        projects::ProjectSubcommand,
        root::{Cli, EntityType},
    },
    config::config,
    models::config::UpdateConfig,
};

pub fn handle_cli(args: Cli) {
    debug!("args: {:?}", args);

    let config = config::get_config();
    debug!("config: {:?}", config);

    if let Ok(_) = config {
        match args.entity_type {
            EntityType::Project(cmd) => match cmd.subcommand {
                ProjectSubcommand::List(_) => {
                    todo!("List projects")
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
        println!("Error reading/creating config:\n{}", err);
    }
}
