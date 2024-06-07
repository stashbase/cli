use crate::{
    cmd::configs::{ConfigCommands, ConfigSubcommand, SetConfigSubcommand},
    handlers::config::set::{
        set_api_key, set_default_output_format, set_default_output_format_secrets,
    },
};

pub async fn handle_config_commands(cmd: ConfigCommands) {
    match cmd.subcommand {
        ConfigSubcommand::Set(args) => match args.subcommand {
            SetConfigSubcommand::ApiKey(t) => {
                set_api_key(t.value);
            }
            SetConfigSubcommand::Output(d) => {
                set_default_output_format(d.format);
            }
            SetConfigSubcommand::OutputSecrets(args) => {
                set_default_output_format_secrets(args.format);
            }
        },
    }
}
