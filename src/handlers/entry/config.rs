use crate::{
    cmd::configs::{
        ApiKeySubcommand, ConfigCommands, ConfigSubcommand, OutputSubcommand,
        SecretsOutputSubcommand,
    },
    handlers::config::{
        api_key,
        ouptut::{print_default_output_format, set_default_output_format},
        set::set_default_output_format_secrets,
    },
    models::config::Config,
};

pub fn handle_config_commands(cmd: ConfigCommands, config: &Config) {
    match cmd.subcommand {
        ConfigSubcommand::ApiKey(k) => match k.subcommand {
            ApiKeySubcommand::Set(s) => {
                api_key::set_api_key(s.value);
            }
            ApiKeySubcommand::Print(p) => {
                api_key::print_api_key(&config.api_key, p.full);
            }
        },
        ConfigSubcommand::Output(o) => match o.subcommand {
            OutputSubcommand::Set(s) => {
                set_default_output_format(s.format);
            }
            OutputSubcommand::Print => {
                if let Some(config) = &config.ouput_format {
                    if let Some(format) = &config.general {
                        print_default_output_format(format);
                    } else {
                        eprintln!("{}", "Default output format is not set");
                    }
                } else {
                    eprintln!("{}", "Default output format is not set");
                }
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
