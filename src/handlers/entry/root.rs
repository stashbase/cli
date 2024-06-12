use log::debug;

use crate::{
    cmd::{
        config::{ConfigSubcommand, OutputFormat, SecretsOutputFormat},
        root::{Cli, EntityType},
    },
    config::config,
    handlers::{
        entry::{
            config::handle_config_commands, environments::handle_environment_commands,
            projects::handle_project_commands, secrets::handle_secrets_commands,
            state::handle_state_commands, webhooks::handle_webhook_commands,
        },
        open::handle_open_dashboard,
        pull::entry::{handle_pull, HandlePullArgs},
        run::entry::{handle_load_env_run, HandleRunArgs},
    },
    models::config::Config,
};

#[tokio::main()]
pub async fn handle_cli(args: Cli) {
    debug!("args: {:?}", args);

    let config = config::get_config();
    debug!("config: {:?}", config);

    if let Ok(mut config) = config {
        if let EntityType::Config(cmd) = args.entity_type {
            if let Err(err) = handle_config_commands(cmd, &config) {
                eprintln!("{:?}", err);
            }

            return;
        }

        if let EntityType::State(cmd) = args.entity_type {
            if let Err(err) = handle_state_commands(cmd, &mut config) {
                eprintln!("{:?}", err);
            }

            return;
        }

        // TODO: check api_key for api commands
        let api_key = match args.api_key {
            Some(api_key) => api_key,
            None => config.api_key.unwrap(),
        };

        let raw_output = args.raw;

        let result = match args.entity_type {
            EntityType::Project(cmd) => {
                let default_output_format = match config.ouput_format {
                    Some(o) => o.general,
                    None => None,
                };
                handle_project_commands(cmd, api_key, raw_output, default_output_format).await
            }
            EntityType::Environment(cmd) => {
                let default_output_format = match config.ouput_format {
                    Some(o) => o.general,
                    None => None,
                };
                handle_environment_commands(
                    cmd,
                    api_key,
                    raw_output,
                    default_output_format,
                    &config.state,
                )
                .await
            }
            EntityType::Config(_) => {
                unreachable!()
            }

            EntityType::Secret(cmd) => {
                // if no secrets output format is set use the general output format
                let default_secrets_output_format = match config.ouput_format {
                    Some(o) => match o.secrets {
                        Some(s) => Some(s),
                        None => match o.general {
                            Some(g) => match g {
                                OutputFormat::List => Some(SecretsOutputFormat::List),
                                OutputFormat::Table => Some(SecretsOutputFormat::Table),
                                OutputFormat::Json => Some(SecretsOutputFormat::Json),
                            },
                            None => None,
                        },
                    },
                    None => None,
                };
                //

                handle_secrets_commands(
                    cmd,
                    api_key,
                    raw_output,
                    default_secrets_output_format,
                    &config.state,
                )
                .await
            }
            EntityType::Webhooks(cmd) => {
                let default_output_format = match config.ouput_format {
                    Some(o) => o.general,
                    None => None,
                };
                handle_webhook_commands(
                    cmd,
                    api_key,
                    raw_output,
                    default_output_format,
                    &config.state,
                )
                .await
            }
            EntityType::Run(args) => {
                let args = HandleRunArgs {
                    api_key,
                    project: args.project,
                    environment: args.environment,
                    command: args.command,
                    exclude: args.exclude,
                    only: args.only,
                    set: args.set,
                    print_secrets: args.print_secrets,
                    file: args.file,
                };

                handle_load_env_run(args).await
            }
            EntityType::Pull(args) => {
                let args = HandlePullArgs {
                    api_key,
                    file: args.config_file,
                    set: args.set,
                    output_file: args.output_file,
                    format: args.format,
                    only: args.only,
                    exclude: args.exclude,
                    print_secrets: args.print_secrets,
                };

                handle_pull(args).await
            }
            EntityType::State(_) => {
                unreachable!()
            }
            EntityType::Open => handle_open_dashboard(api_key).await,
        };

        if let Err(err) = result {
            eprintln!("{:?}", err);
        }
    } else {
        if let EntityType::Config(cmd) = args.entity_type {
            if let ConfigSubcommand::Reset = cmd.subcommand {
                if let Err(e) = handle_config_commands(cmd, &Config::new()) {
                    eprintln!("{:?}", e);
                }

                return;
            }
        }

        let err = config.unwrap_err();
        eprintln!("{:?}", err);
    }
}
