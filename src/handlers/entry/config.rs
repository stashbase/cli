use crate::{
    cmd::configs::{ConfigCommands, ConfigSubcommand, SetConfigSubcommand},
    handlers::config::set::set_token,
};

pub async fn handle_config_commands(cmd: ConfigCommands) {
    match cmd.subcommand {
        ConfigSubcommand::Set(args) => match args.subcommand {
            SetConfigSubcommand::Token(t) => {
                set_token(t.value);
            }
        },
    }
}
