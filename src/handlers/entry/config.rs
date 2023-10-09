use crate::{
    cmd::configs::{ConfigCommands, ConfigSubcommand, SetConfigSubcommand},
    config::config,
    models::config::UpdateConfig,
};

pub async fn handle_config_commands(cmd: ConfigCommands) {
    match cmd.subcommand {
        ConfigSubcommand::Set(args) => match args.subcommand {
            SetConfigSubcommand::Token(t) => {
                config::update_config(UpdateConfig {
                    token: Some(t.value),
                })
                .unwrap();
            }
        },
    }
}
