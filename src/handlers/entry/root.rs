use std::sync::atomic::Ordering;

use crate::{
    cmd::{
        config::{ConfigSubcommand, OutputFormat, SecretsOutputFormat},
        root::{Cli, EntityType, WhoamiCommand, WhoamiOutputFormat},
    },
    config::{config, secure_store},
    handlers::{
        entry::{
            auth::{handle_whoami_command, GetCurrentAuthDetailsRequestArgs},
            config::handle_config_commands,
            environments::handle_environment_commands,
            generate::handle_generate_command,
            projects::handle_project_commands,
            scans::handle_scan_commands,
            secrets::handle_secrets_commands,
            webhooks::handle_webhook_commands,
        },
        open::handle_open_dashboard,
        pull::entry::{handle_pull, HandlePullArgs},
        push::entry::{handle_push, HandlePushArgs},
        run::entry::{handle_load_env_run, HandleRunArgs},
        setup::setup,
    },
    models::{config::Config, validation::InputValidationError},
    utils::{env::get_stashbase_api_key, output::ColorizeIfColoredOutput},
    REQUEST_ABORTED,
};

#[tokio::main()]
pub async fn handle_cli(args: Cli) {
    if let EntityType::Generate(cmd) = args.entity_type {
        if let Err(e) = handle_generate_command(cmd, args.raw) {
            eprintln!("{:?}", e);
        }
        return;
    }

    let config = config::get_config();
    if let Ok(config) = config {
        if let EntityType::Config(cmd) = args.entity_type {
            if let Err(err) = handle_config_commands(cmd, &config) {
                eprintln!("{:?}", err);
            }

            return;
        } else if let EntityType::Setup(_) = args.entity_type {
            if let Err(err) = setup(config) {
                eprintln!("{:?}", err);
            }

            return;
        }

        let secure_store_api_key = secure_store::get_api_key().ok().flatten();
        let mut legacy_config_api_key = config.api_key.clone();

        if secure_store_api_key.is_none() {
            if let Some(legacy_key) = legacy_config_api_key.clone() {
                if secure_store::set_api_key(&legacy_key).is_ok() {
                    let _ = config::clear_legacy_api_key();
                    legacy_config_api_key = None;
                }
            }
        }

        let api_key = args
            .api_key
            .or_else(|| get_stashbase_api_key())
            .or(secure_store_api_key)
            .or(legacy_config_api_key);

        let raw_output = args.raw;
        let silent = args.silent;

        let requires_api_key = args.entity_type.requires_api_key();

        if requires_api_key && api_key.is_none() {
            let error = InputValidationError::MissingApiKey;
            match args.raw {
                false => {
                    if !silent {
                        eprintln!()
                    }

                    eprintln!("{}", error);
                }
                true => {
                    let json_str = error.format_error_output(args.raw).unwrap();
                    if !silent {
                        eprintln!()
                    }

                    eprintln!("{}", json_str);
                }
            }
            return;
        }

        let api_key = api_key.unwrap();

        let result = match args.entity_type {
            EntityType::Whoami(WhoamiCommand { format }) => {
                let format = match format {
                    Some(format) => match format {
                        WhoamiOutputFormat::Json => OutputFormat::Json,
                        WhoamiOutputFormat::Table => OutputFormat::Table,
                        WhoamiOutputFormat::List => OutputFormat::List,
                    },
                    None => match (raw_output, config.ouput_format.and_then(|o| o.general)) {
                        (true, _) => OutputFormat::Json,
                        (false, Some(OutputFormat::Json)) => OutputFormat::Json,
                        (false, Some(OutputFormat::Table)) => OutputFormat::Table,
                        (false, Some(OutputFormat::List)) => OutputFormat::List,
                        _ => OutputFormat::List,
                    },
                };

                let args = GetCurrentAuthDetailsRequestArgs {
                    api_key,
                    format,
                    silent,
                };

                handle_whoami_command(args).await
            }
            EntityType::Project(cmd) => {
                let default_output_format = match config.ouput_format {
                    Some(o) => o.general,
                    None => None,
                };
                handle_project_commands(cmd, api_key, raw_output, silent, default_output_format)
                    .await
            }
            EntityType::Environment(cmd) => {
                let default_output_format = match config.ouput_format {
                    Some(o) => o.general,
                    None => None,
                };
                handle_environment_commands(cmd, api_key, raw_output, silent, default_output_format)
                    .await
            }
            EntityType::Config(_) => {
                unreachable!()
            }
            EntityType::Setup(_) => {
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
                    config.expand_refs,
                    silent,
                    default_secrets_output_format,
                )
                .await
            }
            EntityType::Webhooks(cmd) => {
                let default_output_format = match config.ouput_format {
                    Some(o) => o.general,
                    None => None,
                };
                handle_webhook_commands(cmd, api_key, silent, raw_output, default_output_format)
                    .await
            }
            EntityType::Run(run_cmd) => {
                // Validate scope conflicts
                if let Err(err) = run_cmd.validate_scope_conflicts() {
                    match err.format_error_output(raw_output) {
                        Ok(formatted_err) => {
                            if !silent {
                                eprintln!();
                            }

                            eprintln!("{}", formatted_err);
                            return;
                        }
                        Err(format_err) => {
                            eprintln!("Error formatting validation error: {:?}", format_err);
                            return;
                        }
                    }
                }

                let args = HandleRunArgs {
                    api_key,
                    project: run_cmd.project,
                    environment: run_cmd.environment,
                    command: run_cmd.command,
                    exclude: run_cmd.exclude,
                    only: run_cmd.only,
                    set: run_cmd.set,
                    file: run_cmd.config_file,
                    expand_refs: run_cmd.expand_refs,
                    print_secrets: run_cmd.print_secrets,
                    json_format: raw_output,
                    silent,
                    scope: run_cmd.scope,
                };

                handle_load_env_run(args).await
            }
            EntityType::Pull(pull_cmd) => {
                // Validate scope conflicts
                if let Err(e) = pull_cmd.validate_scope_conflicts() {
                    if !silent {
                        eprintln!();
                    }

                    eprintln!(
                        "{}",
                        e.format_error_output(raw_output)
                            .unwrap_or_else(|_| "Error formatting validation error".to_string())
                    );
                    return;
                }

                let args = HandlePullArgs {
                    api_key,
                    scope: pull_cmd.scope,
                    file: pull_cmd.config_file,
                    set: pull_cmd.set,
                    target_file: pull_cmd.file,
                    format: pull_cmd.format,
                    only: pull_cmd.only,
                    exclude: pull_cmd.exclude,
                    expand_refs: pull_cmd.expand_refs,
                    ignore_comments: pull_cmd.ignore_comments,
                    overwrite_file: pull_cmd.overwrite,
                    json_format: raw_output,
                    silent,
                };

                handle_pull(args).await
            }

            EntityType::Push(push_cmd) => {
                // Validate scope conflicts
                if let Err(e) = push_cmd.validate_scope_conflicts() {
                    if !silent {
                        eprintln!();
                    }

                    eprintln!(
                        "{}",
                        e.format_error_output(raw_output)
                            .unwrap_or_else(|_| "Error formatting validation error".to_string())
                    );
                    return;
                }

                let args = HandlePushArgs {
                    api_key,
                    scope: push_cmd.scope,
                    config_file_path: push_cmd.config_file,
                    target_file: push_cmd.file,
                    format: push_cmd.format,
                    only: push_cmd.only,
                    exclude: push_cmd.exclude,
                    set: push_cmd.set,
                    expand_refs: push_cmd.expand_refs,
                    ignore_comments: push_cmd.ignore_comments,
                    json_format: raw_output,
                    silent,
                };

                handle_push(args).await
            }
            EntityType::Scan(cmd) => handle_scan_commands(cmd, api_key, raw_output, silent).await,
            EntityType::Open => handle_open_dashboard(api_key, silent).await,
            EntityType::Generate(_) => unreachable!(),
        };

        if let Err(err) = result {
            if REQUEST_ABORTED.load(Ordering::SeqCst) {
                eprintln!("{}", "Request aborted".red_if_tty_stderr());
                return;
            }
            eprintln!("{:?}", err);
        }
    } else {
        if let EntityType::Config(cmd) = args.entity_type {
            if let ConfigSubcommand::Reset(_) = cmd.subcommand {
                if let Err(e) = handle_config_commands(cmd, &Config::new()) {
                    eprintln!("{:?}", e);
                }

                return;
            }
        }

        let err = config.unwrap_err();
        if REQUEST_ABORTED.load(Ordering::SeqCst) {
            eprintln!("{}", "Request aborted".red_if_tty_stderr());
            return;
        }
        eprintln!("{:?}", err);
    }
}
