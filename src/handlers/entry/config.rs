use anyhow::Result;

use crate::{
    cmd::config::{
        ApiKeySubcommand, ConfigCommand, ConfigSubcommand, ExpandRefsSubcommand, OutputSubcommand,
        ScopeSubcommand, SecretsOutputSubcommand,
    },
    handlers::config::{
        api_key::{self, get_first_3_and_last_5},
        expand_refs::{print_expand_refs_config, set_expand_refs_config},
        output::{print_default_output_format, set_default_output_format},
        output_secrets::{print_default_secrets_output_format, set_default_secrets_output_format},
        reset::reset_config,
        scope::{print_scope, set_scope},
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
        ConfigSubcommand::Scope(s) => match s.subcommand {
            ScopeSubcommand::Set(s) => {
                set_scope(s.scope);
            }
            ScopeSubcommand::Print => {
                print_scope(&config.clone().scope.unwrap_or_default());
            }
        },
        ConfigSubcommand::Print(args) => {
            if config.is_empty() {
                eprintln!("Config file is empty.");
            } else {
                let mut config_clone = config.clone();

                if !args.show_sensitive {
                    if let Some(api_key) = &config.api_key {
                        let masked_api_key = get_first_3_and_last_5(api_key);

                        match masked_api_key {
                            Some(masked) => {
                                config_clone.api_key = Some(format!("{}...{}", masked.0, masked.1));
                            }
                            None => {
                                config_clone.api_key = Some(String::from("***"));
                            }
                        }
                    }
                }

                let toml_string = toml::to_string(&config_clone);

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
        ConfigSubcommand::Reset(r) => {
            reset_config(r.force)?;
        }
        ConfigSubcommand::ExpandRefs(r) => match r.subcommand {
            ExpandRefsSubcommand::Set(args) => {
                set_expand_refs_config(args.enabled);
            }
            ExpandRefsSubcommand::Print => print_expand_refs_config(&config.expand_refs),
        },
    }

    Ok(())
}
