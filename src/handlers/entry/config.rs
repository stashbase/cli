use crate::{
    cmd::configs::{ConfigCommands, ConfigSubcommand, SetConfigSubcommand},
    handlers::config::set::set_api_key,
};

pub async fn handle_config_commands(cmd: ConfigCommands) {
    match cmd.subcommand {
        ConfigSubcommand::Set(args) => match args.subcommand {
            SetConfigSubcommand::ApiKey(t) => {
                set_api_key(t.value);
            }
        },
    }
}
