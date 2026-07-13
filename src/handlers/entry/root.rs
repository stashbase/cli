use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

use crate::{
    cmd::{
        agent::{AgentProfileSource, AgentSubcommand},
        config::{ConfigSubcommand, OutputFormat, SecretsOutputFormat},
        root::{Cli, EntityType, WhoamiCommand, WhoamiOutputFormat},
    },
    config::{config, secure_store},
    handlers::{
        doctor::handle_doctor_command,
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
        run::{
            broker::{AuditLog, BrokerPolicy, SecretInjection},
            entry::{handle_load_env_run, HandleRunArgs},
            subprocess::CommandFailed,
        },
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

    if let EntityType::Doctor(cmd) = args.entity_type {
        match handle_doctor_command(cmd, args.raw, args.api_key).await {
            Ok(has_failures) => {
                if has_failures {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
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

        let secure_store_api_key = match secure_store::get_api_key() {
            Ok(key) => key,
            Err(err) => {
                if !args.silent {
                    eprintln!(
                        "{} {}",
                        "Warning:".yellow_if_tty_stderr(),
                        format!("Secure key storage is unavailable ({err}).")
                    );
                }
                None
            }
        };
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
                        WhoamiOutputFormat::Plain => OutputFormat::Plain,
                    },
                    None => match (raw_output, config.ouput_format.and_then(|o| o.general)) {
                        (true, _) => OutputFormat::Json,
                        (false, Some(OutputFormat::Json)) => OutputFormat::Json,
                        (false, Some(OutputFormat::Table)) => OutputFormat::Table,
                        (false, Some(OutputFormat::Plain)) => OutputFormat::Plain,
                        _ => OutputFormat::Plain,
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
                                OutputFormat::Plain => Some(SecretsOutputFormat::Plain),
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
            EntityType::Agent(agent_cmd) => match agent_cmd.subcommand {
                AgentSubcommand::Run(agent_run) => async {
                    let global_profile = config
                        .agent_profiles
                        .as_ref()
                        .and_then(|profiles| profiles.get(&agent_run.profile))
                        .cloned();
                    let profile = match agent_run.profile_source {
                        AgentProfileSource::Global => global_profile,
                        AgentProfileSource::Directory => {
                            config::get_directory_agent_profile(&agent_run.profile)?
                        }
                        AgentProfileSource::Auto => config::get_directory_agent_profile(
                            &agent_run.profile,
                        )?
                        .or(global_profile),
                    };

                    let Some(profile) = profile else {
                        let source = match agent_run.profile_source {
                            AgentProfileSource::Global => "global",
                            AgentProfileSource::Directory => "directory",
                            AgentProfileSource::Auto => "global or directory",
                        };
                        eprintln!(
                            "Agent profile '{}' was not found in the {source} config.",
                            agent_run.profile,
                        );
                        return Ok(());
                    };

                    if profile.secrets.is_empty() {
                        eprintln!(
                            "Agent profile '{}' does not grant any secrets.",
                            agent_run.profile
                        );
                        return Ok(());
                    }

                    let valid_source = matches!(
                        (&profile.file, &profile.project, &profile.environment),
                        (Some(_), None, None) | (None, Some(_), Some(_))
                    );
                    if !valid_source {
                        eprintln!(
                            "Agent profile '{}' must define either 'file' or both 'project' and 'environment'.",
                            agent_run.profile
                        );
                        return Ok(());
                    }

                    let policy = BrokerPolicy {
                        allowed_hosts_by_secret: profile
                            .secrets
                            .iter()
                            .map(|(name, secret)| {
                                (
                                    name.clone(),
                                    secret.hosts.iter().cloned().collect::<HashSet<_>>(),
                                )
                            })
                            .collect::<HashMap<_, _>>(),
                        secret_injections: profile
                            .secrets
                            .iter()
                            .filter_map(|(name, secret)| {
                                if secret.header.is_none() && secret.value_template.is_none() {
                                    return None;
                                }
                                let header = secret
                                    .header
                                    .clone()
                                    .unwrap_or_else(|| "authorization".to_owned());
                                let default_template = if header.eq_ignore_ascii_case("authorization") {
                                    "Bearer {secret}"
                                } else {
                                    "{secret}"
                                };
                                Some((
                                    name.clone(),
                                    SecretInjection {
                                        value_template: secret
                                            .value_template
                                            .clone()
                                            .unwrap_or_else(|| default_template.to_owned()),
                                        header,
                                    },
                                ))
                            })
                            .collect(),
                        allowed_egress_hosts: profile
                            .egress_hosts
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .collect(),
                        strict_deny: true,
                    };
                    let audit_log = agent_run
                        .audit_log
                        .then(|| AuditLog::local(&agent_run.profile))
                        .transpose()?;
                    if let Some(audit_log) = &audit_log {
                        if !silent {
                            eprintln!("Audit log: {}", audit_log.path().display());
                        }
                    }
                    let args = HandleRunArgs {
                        api_key,
                        project: profile.project,
                        environment: profile.environment,
                        command: agent_run.command,
                        broker: true,
                        broker_policy: Some(policy),
                        trust_broker_ca: agent_run.trust_broker_ca,
                        sandbox: agent_run.sandbox,
                        audit_log,
                        only: profile.secrets.keys().cloned().collect(),
                        exclude: Vec::new(),
                        set: Vec::new(),
                        set_comments: Vec::new(),
                        print_secrets: None,
                        no_print_secrets: true,
                        config_file: None,
                        file: profile.file,
                        expand_refs: None,
                        json_format: raw_output,
                        silent,
                        scope: None,
                    };
                    handle_load_env_run(args).await
                }
                .await,
            },
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
                    broker: run_cmd.broker,
                    broker_policy: None,
                    trust_broker_ca: false,
                    sandbox: false,
                    audit_log: None,
                    exclude: run_cmd.exclude,
                    only: run_cmd.only,
                    set: run_cmd.set,
                    set_comments: run_cmd.set_comment,
                    config_file: run_cmd.config_file,
                    file: run_cmd.file,
                    expand_refs: run_cmd.expand_refs,
                    print_secrets: run_cmd.print_secrets,
                    no_print_secrets: run_cmd.no_print_secrets,
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
                    set_comments: pull_cmd.set_comment,
                    target_file: pull_cmd.file,
                    format: pull_cmd.format,
                    only: pull_cmd.only,
                    exclude: pull_cmd.exclude,
                    expand_refs: pull_cmd.expand_refs,
                    ignore_comments: pull_cmd.ignore_comments,
                    print_secrets: pull_cmd.print_secrets,
                    no_print_secrets: pull_cmd.no_print_secrets,
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
                    set_comments: push_cmd.set_comment,
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
            EntityType::Doctor(_) => unreachable!(),
        };

        if let Err(err) = result {
            if REQUEST_ABORTED.load(Ordering::SeqCst) {
                eprintln!("{}", "Request aborted".red_if_tty_stderr());
                return;
            }
            eprintln!("{:?}", err);
            if let Some(command_failed) = err.downcast_ref::<CommandFailed>() {
                std::process::exit(command_failed.exit_code());
            }
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
