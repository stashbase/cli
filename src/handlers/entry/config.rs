use crate::{
    cmd::configs::{
        ApiKeySubcommand, ConfigCommands, ConfigSubcommand, OutputSubcommand,
        SecretsOutputSubcommand,
    },
    handlers::config::{
        api_key,
        output::{print_default_output_format, set_default_output_format},
        output_secrets::{print_default_secrets_output_format, set_default_output_format_secrets},
    },
    models::config::Config,
};

fn print_output_format_not_set() {
    eprintln!("{}", "Default output format is not set");
}

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
                        print_output_format_not_set()
                    }
                } else {
                    print_output_format_not_set()
                }
            }
        },
        ConfigSubcommand::OutputSecrets(s) => match s.subcommand {
            SecretsOutputSubcommand::Set(s) => {
                set_default_output_format_secrets(s.format);
            }
            SecretsOutputSubcommand::Print => {
                if let Some(config) = &config.ouput_format {
                    if let Some(format) = &config.secrets {
                        print_default_secrets_output_format(format);
                    } else {
                        print_output_format_not_set()
                    }
                } else {
                    print_output_format_not_set()
                }
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
