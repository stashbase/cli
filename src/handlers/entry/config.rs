use anyhow::Result;

use crate::{
    cmd::config::{
        ApiKeySubcommand, ConfigCommand, ConfigSubcommand, OutputSubcommand, ReplaceRefsSubcommand,
        SecretsOutputSubcommand,
    },
    handlers::config::{
        api_key,
        output::{print_default_output_format, set_default_output_format},
        output_secrets::{print_default_secrets_output_format, set_default_secrets_output_format},
        reset::reset_config,
        resolve_refs::{print_replace_refs_config, set_replace_refs_config},
    },
    models::config::Config,
};

fn print_output_format_not_set() {
    eprintln!("{}", "Default output format is not set");
}

pub fn handle_config_commands(cmd: ConfigCommand, config: &Config) -> Result<()> {
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
                set_default_secrets_output_format(s.format);
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
        ConfigSubcommand::Print => {
            if config.is_empty() {
                eprintln!("Config file is empty");
            } else {
                let toml_string = toml::to_string(&config);

                match toml_string {
                    Ok(s) => {
                        eprintln!();
                        print!("{}", s);
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
        }
        ConfigSubcommand::Reset => {
            if let Err(e) = reset_config() {
                return Err(e);
            };
        }
        ConfigSubcommand::ReplaceRefs(r) => match r.subcommand {
            ReplaceRefsSubcommand::Set(args) => {
                set_replace_refs_config(args.enabled);
            }
            ReplaceRefsSubcommand::Print => print_replace_refs_config(&config.resolve_refs),
        },
    }

    Ok(())
}
