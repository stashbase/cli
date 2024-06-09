use crate::{
    cmd::configs::{
        ApiKeySubcommand, ConfigCommands, ConfigSubcommand, OutputSubcommand,
        SecretsOutputSubcommand,
    },
    handlers::config::set::{
        set_api_key, set_default_output_format, set_default_output_format_secrets,
    },
};

pub async fn handle_config_commands(cmd: ConfigCommands) {
    match cmd.subcommand {
        ConfigSubcommand::ApiKey(k) => match k.subcommand {
            ApiKeySubcommand::Set(s) => {
                set_api_key(s.value);
            }
        },
        ConfigSubcommand::Output(o) => match o.subcommand {
            OutputSubcommand::Set(s) => {
                set_default_output_format(s.format);
            }
        },
        ConfigSubcommand::OutputSecrets(s) => match s.subcommand {
            SecretsOutputSubcommand::Set(s) => {
                set_default_output_format_secrets(s.format);
            }
        },
    }
}
// }
// pub async fn handle_config_commands(cmd: ConfigCommands) {
//     match cmd.subcommand {
//         ConfigSubcommand::Set(args) => match args.subcommand {
//             SetConfigSubcommand::ApiKey(t) => {
//                 set_api_key(t.value);
//             }
//             SetConfigSubcommand::Output(d) => {
//                 set_default_output_format(d.format);
//             }
//             SetConfigSubcommand::OutputSecrets(args) => {
//                 set_default_output_format_secrets(args.format);
//             }
//         },
//     }
// }
