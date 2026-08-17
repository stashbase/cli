use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::atomic::Ordering,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::Serialize;
use tabled::Tabled;
use tokio::{sync::watch, task::JoinHandle};

use crate::{
    cmd::{
        agent::{
            AgentAuditGroupBy, AgentLogsCommand, AgentLogsListCommand, AgentLogsSubcommand,
            AgentLogsSummaryCommand, AgentProfileSource, AgentSubcommand,
        },
        config::{ConfigSubcommand, OutputFormat, SecretsOutputFormat},
        root::{Cli, EntityType, WhoamiCommand, WhoamiOutputFormat},
    },
    config::{config, secure_store},
    handlers::{
        agent_doctor::handle_agent_doctor_command,
        agent_explain::handle_agent_explain_command,
        agent_init::handle_agent_init_command,
        agent_policy::SecretHttpPolicy,
        agent_policy_test::handle_agent_policy_test_command,
        agent_profiles::handle_agent_profiles_command,
        agent_validate::handle_agent_validate_command,
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
            entry::{handle_load_env_run, handle_remote_agent_run, HandleRunArgs},
            proxy::{
                read_local_proxy_audit_logs, ProfileAuditProvenance, ProxyAuditLog,
                ProxyAuditLogEvent, ProxyAuditLogFilter, ProxyPolicy, SecretInjection,
            },
            subprocess::CommandFailed,
        },
        setup::setup,
    },
    models::{config::Config, validation::InputValidationError},
    utils::{
        env::get_stashbase_api_key, output::ColorizeIfColoredOutput, tables::build::build_table,
    },
    REQUEST_ABORTED,
};

#[cfg(unix)]
fn install_remote_agent_shutdown_handler() {
    use tokio::signal::unix::{signal, SignalKind};

    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        return;
    };
    let Ok(mut hangup) = signal(SignalKind::hangup()) else {
        return;
    };
    let Ok(mut interrupt) = signal(SignalKind::interrupt()) else {
        return;
    };
    tokio::spawn(async move {
        let exit_code = tokio::select! {
            _ = terminate.recv() => 143,
            _ = hangup.recv() => 129,
            _ = interrupt.recv() => 130,
        };
        crate::api::remote_proxy::end_registered_agent_run().await;
        std::process::exit(exit_code);
    });
}

#[cfg(not(unix))]
fn install_remote_agent_shutdown_handler() {
    // Windows has no Unix-style SIGTERM/SIGHUP handling here, but Ctrl+C is the
    // normal interactive termination path. End the remote session before exit.
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            crate::api::remote_proxy::end_registered_agent_run().await;
            std::process::exit(130);
        }
    });
}

/// Classifies only the executable basename for remote-session metadata. Never
/// forward the command path, prompt, or arguments to the control plane.
fn infer_remote_agent_type(command: &[String]) -> &'static str {
    let executable = command.first().map(String::as_str).unwrap_or_default();
    let basename = Path::new(executable)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(executable)
        .to_ascii_lowercase();
    match basename.as_str() {
        "codex" => "codex",
        "claude" | "claude-code" => "claude-code",
        "copilot" | "github-copilot" => "copilot",
        "cursor" | "cursor-agent" => "cursor",
        "opencode" => "opencode",
        _ => "custom",
    }
}

fn secret_child_name(
    name: &str,
    secret: &crate::models::agent::AgentSecretProfile,
    is_remote: bool,
) -> String {
    if is_remote {
        name.to_owned()
    } else {
        secret.env.clone().unwrap_or_else(|| name.to_owned())
    }
}

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

        // Local commands such as `agent logs` do not need Stashbase authentication.
        let api_key = api_key.unwrap_or_default();

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
                AgentSubcommand::Init(agent_init) => {
                    handle_agent_init_command(agent_init, silent, raw_output)
                }
                AgentSubcommand::Logs(mut agent_logs) => match agent_logs.subcommand.take() {
                    Some(AgentLogsSubcommand::List(list)) => {
                        handle_agent_logs(list.into(), raw_output).await
                    }
                    Some(AgentLogsSubcommand::Summary(summary)) => {
                        handle_agent_logs_summary(summary, silent, raw_output)
                    }
                    None => handle_agent_logs(agent_logs, raw_output).await,
                },
                AgentSubcommand::Doctor(agent_doctor) => {
                    match handle_agent_doctor_command(agent_doctor, raw_output).await {
                        Ok(true) => std::process::exit(1),
                        Ok(false) => Ok(()),
                        Err(error) => Err(error),
                    }
                }
                AgentSubcommand::Validate(agent_validate) => {
                    match handle_agent_validate_command(agent_validate, &config, raw_output).await {
                        Ok(true) => std::process::exit(1),
                        Ok(false) => Ok(()),
                        Err(error) => Err(error),
                    }
                }
                AgentSubcommand::Explain(agent_explain) => {
                    handle_agent_explain_command(agent_explain, &config, silent, raw_output)
                }
                AgentSubcommand::Policy(agent_policy) => match agent_policy.subcommand {
                    crate::cmd::agent::AgentPolicySubcommand::Test(agent_policy_test) => {
                        match handle_agent_policy_test_command(
                            agent_policy_test,
                            &config,
                            silent,
                            raw_output,
                        ) {
                            Ok(true) => std::process::exit(1),
                            Ok(false) => Ok(()),
                            Err(error) => Err(error),
                        }
                    }
                },
                AgentSubcommand::Profiles(agent_profiles) => {
                    handle_agent_profiles_command(agent_profiles, &config, silent, raw_output)
                }
                AgentSubcommand::Run(agent_run) => async {
                    let explicit_profile = agent_run
                        .policy_file
                        .as_deref()
                        .map(config::get_explicit_agent_profile)
                        .transpose()?;
                    let global_profile = config
                        .agent_profiles
                        .as_ref()
                        .and_then(|profiles| profiles.get(&agent_run.profile))
                        .cloned();
                    let (profile, directory_source, profile_path) = if let Some(profile) = explicit_profile {
                        (
                            Some(profile.profile),
                            Some(profile.source),
                            Some(profile.path),
                        )
                    } else { match agent_run.profile_source {
                        AgentProfileSource::Global => {
                            (global_profile, None, Some(config::get_config_path()?))
                        }
                        AgentProfileSource::Directory => {
                            let profile = config::get_directory_agent_profile(&agent_run.profile)?;
                            let source = profile.as_ref().map(|profile| profile.source.clone());
                            let path = profile.as_ref().map(|profile| profile.path.clone());
                            (profile.map(|profile| profile.profile), source, path)
                        }
                        AgentProfileSource::Auto => {
                            let directory_profile =
                                config::get_directory_agent_profile(&agent_run.profile)?;
                            let source = directory_profile
                                .as_ref()
                                .map(|profile| profile.source.clone());
                            let path = directory_profile.as_ref().map(|profile| profile.path.clone());
                            (
                                directory_profile.map(|profile| profile.profile).or(global_profile),
                                source,
                                path.or(Some(config::get_config_path()?)),
                            )
                        }
                    }};
                    let loaded_from_directory = directory_source.is_some();

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

                    crate::handlers::agent_validate::ensure_profile_is_valid_for_run(&profile)?;

                    if loaded_from_directory
                        && matches!(agent_run.profile_source, AgentProfileSource::Auto)
                        && agent_run.policy_file.is_none()
                        && !silent
                    {
                        eprintln!(
                            "Warning: Loaded agent profile '{}' from {}. Review this repository policy before granting secrets.",
                            agent_run.profile,
                            directory_source.as_deref().unwrap(),
                        );
                    }

                    if loaded_from_directory && !silent {
                        if let Some(warning) = directory_profile_git_warning(
                            profile_path
                                .as_deref()
                                .context("Agent profile source path is unavailable")?,
                            directory_source.as_deref().unwrap(),
                        ) {
                            eprintln!("Warning: {warning}");
                        }
                    }

                    if !silent {
                        if agent_run.sandbox {
                            eprintln!("Network sandbox: enabled");
                        } else {
                            eprintln!(
                                "Warning: Network sandbox is disabled. A tool that bypasses proxy settings may make direct network requests. Enable --sandbox for network containment on supported platforms."
                            );
                        }
                        print_agent_egress_warnings(&profile);
                    }

                    let valid_source = matches!(
                        (&profile.file, &profile.project, &profile.environment),
                        (Some(_), None, None)
                            | (None, Some(_), Some(_))
                            | (Some(_), Some(_), Some(_))
                    );
                    let egress_only = profile.secrets.is_empty();
                    if egress_only && !matches!((&profile.file, &profile.project, &profile.environment), (None, None, None)) {
                        eprintln!(
                            "Egress-only agent profile '{}' must not define 'file', 'project', or 'environment'.",
                            agent_run.profile
                        );
                        return Ok(());
                    }
                    if !egress_only && !valid_source {
                        eprintln!(
                            "Agent profile '{}' must define 'file', both 'project' and 'environment', or both sources together.",
                            agent_run.profile
                        );
                        return Ok(());
                    }
                    if egress_only && !silent {
                        eprintln!(
                            "Warning: Egress-only profile. No Stashbase-managed secrets are granted to this agent."
                        );
                    }

                    let is_remote = agent_run.remote;
                    let secret_bindings = profile
                        .secrets
                        .iter()
                        .map(|(name, secret)| {
                            let child_name = secret_child_name(name, secret, is_remote);
                            (secret.from.clone().unwrap_or_else(|| name.clone()), child_name)
                        })
                        .collect::<HashMap<_, _>>();
                    if secret_bindings.len() != profile.secrets.len() {
                        eprintln!(
                            "Agent profile '{}' maps more than one binding to the same source secret.",
                            agent_run.profile
                        );
                        return Ok(());
                    }

                    let policy = ProxyPolicy {
                        secret_policies: profile
                            .secrets
                            .iter()
                            .map(|(name, secret)| {
                                let policy = if secret.rules.is_empty() {
                                    SecretHttpPolicy::LegacyHosts(
                                        secret.hosts.iter().cloned().collect::<HashSet<_>>(),
                                    )
                                } else {
                                    SecretHttpPolicy::Rules(
                                        secret.rules.clone(),
                                    )
                                };
                                let child_name = secret_child_name(name, secret, is_remote);
                                (child_name, policy)
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
                                    secret_child_name(name, secret, is_remote),
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
                        denied_hosts: profile
                            .deny_hosts
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .collect(),
                        egress_hosts_configured: profile.egress_hosts.is_some(),
                        strict_deny: true,
                    };
                    let policy_fingerprint = policy.fingerprint();
                    let profile_source = directory_source
                        .clone()
                        .unwrap_or_else(|| "user-level config".to_owned());
                    let profile_provenance = ProfileAuditProvenance::from_file(
                        profile_source,
                        profile_path.as_deref().context("Agent profile source path is unavailable")?,
                    )?;
                    let audit_log = (!agent_run.remote)
                        .then(|| {
                            agent_run.audit_log.then(|| {
                                ProxyAuditLog::local(
                                    &agent_run.profile,
                                    policy_fingerprint.clone(),
                                )
                                .map(|audit_log| audit_log.with_profile_provenance(profile_provenance.clone()))
                            })
                        })
                        .flatten()
                        .transpose()?;
                    if let Some(audit_log) = &audit_log {
                        if !silent {
                            eprintln!("Audit session: {}", audit_log.session_id());
                            eprintln!("Policy fingerprint: {}", audit_log.policy_fingerprint());
                            eprintln!("Audit log: {}", audit_log.path().display());
                        }
                    }
                    if agent_run.remote {
                        let (Some(project), Some(environment)) =
                            (profile.project.clone(), profile.environment.clone())
                        else {
                            anyhow::bail!("--remote requires a project/environment-backed agent profile.");
                        };
                        if profile.file.is_some() || profile.secrets.is_empty() {
                            anyhow::bail!("--remote currently supports Stashbase-managed secret bindings, not local-file or egress-only profiles.");
                        }
                        // Keep ordinary egress separate from per-secret destinations. The
                        // control plane applies `egress_hosts` to uncredentialed requests and
                        // each binding's `hosts` only when it injects that secret.
                        let egress_hosts = profile
                            .egress_hosts
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .collect::<Vec<_>>();
                        let deny_hosts = profile.deny_hosts.clone().unwrap_or_default();
                        let bindings = profile.secrets.iter().map(|(name, secret)| {
                            let header = secret.header.clone().unwrap_or_else(|| "authorization".to_owned());
                            let value_template = secret.value_template.clone().unwrap_or_else(|| {
                                if header.eq_ignore_ascii_case("authorization") { "Bearer {secret}".to_owned() } else { "{secret}".to_owned() }
                            });
                            crate::api::remote_proxy::RemoteBinding {
                                name: name.clone(),
                                from: secret.from.clone().unwrap_or_else(|| name.clone()),
                                hosts: secret.hosts.clone(),
                                rules: secret
                                    .rules
                                    .iter()
                                    .cloned()
                                    .map(|mut rule| {
                                        rule.methods = rule
                                            .methods
                                            .into_iter()
                                            .map(|method| method.trim().to_ascii_uppercase())
                                            .collect();
                                        rule
                                    })
                                    .collect(),
                                header,
                                placeholder: secret
                                    .placeholder
                                    .clone()
                                    .unwrap_or_else(|| format!("${{STASHBASE_{name}}}")),
                                value_template,
                            }
                        }).collect::<Vec<_>>();
                        let session_request = crate::api::remote_proxy::RemoteProxySessionRequest {
                            api_key: api_key.clone(),
                            project_identifier: project,
                            environment_identifier: environment,
                            egress_hosts,
                            deny_hosts,
                            bindings: bindings.clone(),
                            agent_type: Some(infer_remote_agent_type(&agent_run.command).to_owned()),
                            previous_session_token: None,
                        };
                        let session = crate::api::remote_proxy::create_session(&session_request, raw_output)
                            .await
                            // The Agent Proxy setup follows startup warnings. Keep its
                            // formatted API error visually distinct without changing
                            // output spacing for every other CLI command.
                            .map_err(|error| anyhow::anyhow!("\n{error}"))?;
                        let token = session.session_token.clone();
                        let remote_audit_log = match agent_run
                            .audit_log
                            .then(|| {
                                ProxyAuditLog::local_with_session_id(
                                    &agent_run.profile,
                                    session.session_id.clone(),
                                    policy_fingerprint.clone(),
                                )
                                .map(|audit_log| audit_log.with_profile_provenance(profile_provenance.clone()))
                            })
                            .transpose()
                        {
                            Ok(audit_log) => audit_log,
                            Err(error) => {
                                crate::api::remote_proxy::revoke_session(api_key.clone(), &token).await;
                                return Err(error);
                            }
                        };
                        if let Some(audit_log) = &remote_audit_log {
                            if !silent {
                                eprintln!("Audit session: {}", audit_log.session_id());
                                eprintln!("Policy fingerprint: {}", audit_log.policy_fingerprint());
                                eprintln!("Audit log: {}", audit_log.path().display());
                            }
                        }
                        let placeholders = bindings
                            .into_iter()
                            .map(|binding| (binding.name, binding.placeholder))
                            .collect();
                        let child_env = profile
                            .secrets
                            .iter()
                            .map(|(name, secret)| {
                                (
                                    name.clone(),
                                    secret.env.clone().unwrap_or_else(|| name.clone()),
                                )
                            })
                            .collect();
                        let protocol = match session.protocol.as_str() {
                            "http/1.1-custom" => crate::handlers::run::proxy::RemoteProxyProtocol::Custom,
                            "http/1.1-forward-proxy-tls-intercept" => crate::handlers::run::proxy::RemoteProxyProtocol::ForwardProxyTlsIntercept,
                            value => {
                                crate::api::remote_proxy::revoke_session(api_key.clone(), &token).await;
                                anyhow::bail!("Agent Proxy returned an unsupported protocol: {value}");
                            }
                        };
                        let remote_ca_file = match provision_remote_session_ca(&session) {
                            Ok(path) => path,
                            Err(error) => {
                                crate::api::remote_proxy::revoke_session(api_key.clone(), &token).await;
                                return Err(error);
                            }
                        };
                        let remote_transport_identity =
                            remote_session_transport_identity(&session)?;
                        let proxy_url = session.proxy_url.clone();
                        let initial_state = match remote_session_state(&session) {
                            Ok(state) => state,
                            Err(error) => {
                                crate::api::remote_proxy::revoke_session(api_key.clone(), &token)
                                    .await;
                                return Err(error);
                            }
                        };
                        let remote_session = Arc::new(RwLock::new(initial_state));
                        crate::api::remote_proxy::register_agent_run_cleanup(
                            api_key.clone(),
                            token.clone(),
                        );
                        // Install signal interception only for a run that has a remote
                        // session to end. Installing it at CLI startup would replace the
                        // normal SIGINT/SIGTERM behavior for every Stashbase command.
                        install_remote_agent_shutdown_handler();
                        let (rotation_stop, rotation_task) = spawn_remote_session_rotation(
                            session_request,
                            session,
                            remote_session.clone(),
                            remote_transport_identity,
                        );
                        let result = handle_remote_agent_run(
                            agent_run.command,
                            policy,
                            crate::handlers::run::proxy::RemoteProxyConfig { proxy_url, session: remote_session.clone(), placeholders, child_env, protocol, ca_file: remote_ca_file },
                            agent_run.proxy_port,
                            agent_run.sandbox,
                            agent_run.trust_proxy_ca,
                            remote_audit_log,
                            secret_bindings.keys().cloned().collect(),
                            silent,
                        ).await;
                        let _ = rotation_stop.send(true);
                        let _ = rotation_task.await;
                        let current_token = remote_session
                            .read()
                            .map(|session| session.token.clone())
                            .unwrap_or(token);
                        crate::api::remote_proxy::end_agent_run(api_key, &current_token).await;
                        crate::api::remote_proxy::clear_agent_run_cleanup();
                        return result;
                    }
                    let args = HandleRunArgs {
                        api_key,
                        project: profile.project,
                        environment: profile.environment,
                        command: agent_run.command,
                        proxy: true,
                        proxy_port: agent_run.proxy_port,
                        proxy_policy: Some(policy),
                        trust_proxy_ca: agent_run.trust_proxy_ca,
                        sandbox: agent_run.sandbox,
                        audit_log,
                        secret_bindings: secret_bindings.clone(),
                        allow_file_override: true,
                        only: secret_bindings.keys().cloned().collect(),
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
                    proxy: run_cmd.proxy,
                    proxy_port: run_cmd.proxy_port,
                    proxy_policy: None,
                    trust_proxy_ca: false,
                    sandbox: false,
                    audit_log: None,
                    secret_bindings: HashMap::new(),
                    allow_file_override: false,
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

async fn handle_agent_logs(command: AgentLogsCommand, json: bool) -> anyhow::Result<()> {
    if command.limit == 0 || command.limit > 1_000 {
        anyhow::bail!("--limit must be between 1 and 1000.");
    }
    let since = command
        .since
        .as_deref()
        .map(parse_audit_duration)
        .transpose()?;
    let filter = ProxyAuditLogFilter {
        profile: command.profile,
        action: command.action,
        host: command.host,
        session: command.session,
        id: command.id,
    };
    if !command.follow {
        let events = read_local_proxy_audit_logs(command.limit, since, &filter)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&events)?);
        } else {
            for event in &events {
                print_audit_event(event, false)?;
            }
        }
        return Ok(());
    }

    let mut displayed = HashSet::new();

    loop {
        if REQUEST_ABORTED.load(Ordering::SeqCst) {
            return Ok(());
        }
        for event in read_local_proxy_audit_logs(command.limit, since, &filter)? {
            if !displayed.insert(event.clone()) {
                continue;
            }
            print_audit_event(&event, json)?;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

impl From<AgentLogsListCommand> for AgentLogsCommand {
    fn from(command: AgentLogsListCommand) -> Self {
        Self {
            subcommand: None,
            limit: command.limit,
            since: command.since,
            profile: command.profile,
            action: command.action,
            host: command.host,
            session: command.session,
            id: command.id,
            follow: command.follow,
        }
    }
}

fn handle_agent_logs_summary(
    command: AgentLogsSummaryCommand,
    silent: bool,
    json: bool,
) -> anyhow::Result<()> {
    if command.limit == 0 || command.limit > 1_000 {
        anyhow::bail!("--limit must be between 1 and 1000.");
    }
    let since = command
        .since
        .as_deref()
        .map(parse_audit_duration)
        .transpose()?;
    let filter = ProxyAuditLogFilter {
        profile: command.profile,
        action: command.action,
        host: command.host,
        session: command.session,
        id: command.id,
    };
    let report = summarize_audit_events(
        read_local_proxy_audit_logs(command.limit, since, &filter)?,
        command.limit,
        command.since.clone(),
        command.group_by,
    );
    if !silent {
        println!();
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Agent proxy audit summary");
    println!(
        "Events: {} (newest matching events; limit {})",
        report.events, command.limit
    );
    println!("Requests: {}", report.requests);
    println!("Injected: {}", report.injected);
    println!("Forwarded without credential: {}", report.forwarded);
    println!("Denied: {}", report.denied);
    println!("Uploaded: {}", format_bytes(report.request_bytes));
    println!("Downloaded: {}", format_bytes(report.response_bytes));
    if !report.groups.is_empty() {
        println!();
        println!("Grouped by {}:", report.group_by.unwrap_or_default());
        print_audit_group_summary_table(&report.groups);
    }
    if !report.denied_by.is_empty() {
        println!();
        println!("Denied by:");
        print_denied_summary_table(&report.denied_by);
    }
    Ok(())
}

fn print_audit_group_summary_table(entries: &[AuditGroupSummary]) {
    print!("{}", format_audit_group_summary_table(entries));
}

fn format_audit_group_summary_table(entries: &[AuditGroupSummary]) -> String {
    let rows = entries
        .iter()
        .map(|entry| AuditGroupSummaryRow {
            group: entry.value.clone(),
            events: entry.events,
            requests: entry.requests,
            denied: entry.denied,
            uploaded: format_bytes(entry.request_bytes),
            downloaded: format_bytes(entry.response_bytes),
        })
        .collect::<Vec<_>>();
    build_table(&rows).to_string() + "\n"
}

fn print_denied_summary_table(entries: &[AuditDeniedSummary]) {
    print!("{}", format_denied_summary_table(entries));
}

fn format_denied_summary_table(entries: &[AuditDeniedSummary]) -> String {
    let rows = entries
        .iter()
        .map(|entry| AuditDeniedSummaryRow {
            action: entry.action.clone(),
            host: entry.host.clone(),
            count: entry.count,
        })
        .collect::<Vec<_>>();
    build_table(&rows).to_string() + "\n"
}

fn summarize_audit_events(
    events: Vec<ProxyAuditLogEvent>,
    limit: usize,
    since: Option<String>,
    group_by: Option<AgentAuditGroupBy>,
) -> AuditSummary {
    let events_count = events.len();
    let requests = events.iter().filter(|event| event.method.is_some()).count();
    let injected = events
        .iter()
        .filter(|event| event.action == "injected")
        .count();
    let forwarded = events
        .iter()
        .filter(|event| event.action == "forwarded")
        .count();
    let request_bytes = events.iter().filter_map(|event| event.request_bytes).sum();
    let response_bytes = events.iter().filter_map(|event| event.response_bytes).sum();
    let denied_events = events
        .iter()
        .filter(|event| event.response_status == Some(403))
        .collect::<Vec<_>>();
    let mut denied_by = BTreeMap::<(String, String), usize>::new();
    for event in &denied_events {
        *denied_by
            .entry((
                event.action.clone(),
                event
                    .destination_host
                    .clone()
                    .unwrap_or_else(|| "-".to_owned()),
            ))
            .or_default() += 1;
    }
    let mut denied_by = denied_by
        .into_iter()
        .map(|((action, host), count)| AuditDeniedSummary {
            action,
            host,
            count,
        })
        .collect::<Vec<_>>();
    denied_by.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.action.cmp(&right.action))
            .then_with(|| left.host.cmp(&right.host))
    });
    let groups = group_by.map(|group_by| summarize_audit_groups(&events, group_by));
    AuditSummary {
        limit,
        since,
        events: events_count,
        requests,
        injected,
        forwarded,
        denied: denied_events.len(),
        request_bytes,
        response_bytes,
        denied_by,
        group_by: group_by.map(audit_group_by_name),
        groups: groups.unwrap_or_default(),
    }
}

#[derive(Serialize)]
struct AuditSummary {
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    events: usize,
    requests: usize,
    injected: usize,
    forwarded: usize,
    denied: usize,
    request_bytes: u64,
    response_bytes: u64,
    denied_by: Vec<AuditDeniedSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_by: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    groups: Vec<AuditGroupSummary>,
}

#[derive(Serialize)]
struct AuditDeniedSummary {
    action: String,
    host: String,
    count: usize,
}

#[derive(Tabled)]
struct AuditDeniedSummaryRow {
    #[tabled(rename = "ACTION")]
    action: String,
    #[tabled(rename = "HOST")]
    host: String,
    #[tabled(rename = "COUNT")]
    count: usize,
}

#[derive(Serialize)]
struct AuditGroupSummary {
    value: String,
    events: usize,
    requests: usize,
    denied: usize,
    request_bytes: u64,
    response_bytes: u64,
}

#[derive(Tabled)]
struct AuditGroupSummaryRow {
    #[tabled(rename = "GROUP")]
    group: String,
    #[tabled(rename = "EVENTS")]
    events: usize,
    #[tabled(rename = "REQUESTS")]
    requests: usize,
    #[tabled(rename = "DENIED")]
    denied: usize,
    #[tabled(rename = "UPLOADED")]
    uploaded: String,
    #[tabled(rename = "DOWNLOADED")]
    downloaded: String,
}

fn audit_group_by_name(group_by: AgentAuditGroupBy) -> &'static str {
    match group_by {
        AgentAuditGroupBy::Host => "host",
        AgentAuditGroupBy::Action => "action",
        AgentAuditGroupBy::Secret => "secret",
    }
}

fn summarize_audit_groups(
    events: &[ProxyAuditLogEvent],
    group_by: AgentAuditGroupBy,
) -> Vec<AuditGroupSummary> {
    let mut groups = BTreeMap::<String, AuditGroupSummary>::new();
    for event in events {
        let value = match group_by {
            AgentAuditGroupBy::Host => event.destination_host.as_deref(),
            AgentAuditGroupBy::Action => Some(event.action.as_str()),
            AgentAuditGroupBy::Secret => event.secret_name.as_deref(),
        }
        .unwrap_or("-")
        .to_owned();
        let group = groups.entry(value.clone()).or_insert(AuditGroupSummary {
            value,
            events: 0,
            requests: 0,
            denied: 0,
            request_bytes: 0,
            response_bytes: 0,
        });
        group.events += 1;
        group.requests += usize::from(event.method.is_some());
        group.denied += usize::from(event.response_status == Some(403));
        group.request_bytes += event.request_bytes.unwrap_or_default();
        group.response_bytes += event.response_bytes.unwrap_or_default();
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        right
            .response_bytes
            .cmp(&left.response_bytes)
            .then_with(|| right.events.cmp(&left.events))
            .then_with(|| left.value.cmp(&right.value))
    });
    groups
}

/// Returns a startup warning when repository-controlled policy has not been
/// committed. A missing repository is normal for local profiles and is silent.
fn directory_profile_git_warning(profile_path: &Path, source: &str) -> Option<String> {
    let repository = git2::Repository::discover(profile_path.parent()?).ok()?;
    let workdir = repository.workdir()?.canonicalize().ok()?;
    let profile_path = profile_path.canonicalize().ok()?;
    let relative_path = profile_path.strip_prefix(workdir).ok()?;
    let is_untracked = repository
        .index()
        .map(|index| index.get_path(relative_path, 0).is_none())
        // A newly initialized repository may not have an index file yet.
        .unwrap_or(true);
    if is_untracked {
        return None;
    }
    let status = repository.status_file(relative_path).ok()?;
    if status.is_empty() {
        return None;
    }
    Some(format!(
        "Agent profile {source} has uncommitted Git changes. Review or commit this policy before granting credentials."
    ))
}

/// Makes the active risk visible at launch time. The proxy remains host-based:
/// allowing the Stashbase API host lets a child use any API route its normal
/// local credential is authorized for.
fn print_agent_egress_warnings(profile: &crate::models::agent::AgentProfile) {
    let Some(api_host) = crate::api::client::get_api_host() else {
        return;
    };
    let api_denied = profile.deny_hosts.as_ref().is_some_and(|hosts| {
        hosts
            .iter()
            .any(|denied| denied == "*" || configured_host_matches(denied, &api_host))
    });
    let unrestricted_egress = profile
        .egress_hosts
        .as_ref()
        .is_some_and(|hosts| hosts.iter().any(|host| host.trim() == "*"));
    if unrestricted_egress && !api_denied {
        eprintln!(
            "Warning: Profile allows unrestricted HTTP(S) egress. The child may reach the Stashbase API and use locally stored normal authentication."
        );
        return;
    }
    let egress_allows_api = profile.egress_hosts.as_ref().is_some_and(|hosts| {
        hosts
            .iter()
            .any(|allowed| configured_host_matches(allowed, &api_host))
    });
    let secret_allows_api = profile.secrets.values().any(|secret| {
        secret
            .hosts
            .iter()
            .any(|allowed| configured_host_matches(allowed, &api_host))
    });
    if !api_denied && (egress_allows_api || secret_allows_api) {
        eprintln!(
            "Warning: Profile allows the Stashbase API host ({api_host}). The child may run normal Stashbase CLI commands using locally stored authentication."
        );
    }
}

fn configured_host_matches(allowed: &str, host: &str) -> bool {
    let allowed = allowed.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    match allowed.strip_prefix("*.") {
        Some(suffix) => host != suffix && host.ends_with(&format!(".{suffix}")),
        None => allowed == host,
    }
}

fn parse_audit_duration(value: &str) -> anyhow::Result<Duration> {
    let split_at = value.find(|character: char| !character.is_ascii_digit());
    let Some(split_at) = split_at else {
        anyhow::bail!("Invalid --since value '{value}'. Use a value such as 30m, 24h, or 7d.");
    };
    let (amount, unit) = value.split_at(split_at);
    let amount = amount.parse::<u64>().map_err(|_| {
        anyhow::anyhow!("Invalid --since value '{value}'. Use a value such as 30m, 24h, or 7d.")
    })?;
    let seconds_per_unit = match unit {
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => anyhow::bail!("Invalid --since value '{value}'. Use a value such as 30m, 24h, or 7d."),
    };
    let seconds = amount.checked_mul(seconds_per_unit).ok_or_else(|| {
        anyhow::anyhow!("Invalid --since value '{value}': duration is too large.")
    })?;
    Ok(Duration::from_secs(seconds))
}

fn print_audit_event(event: &ProxyAuditLogEvent, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string(event)?);
        return Ok(());
    }

    let host = event.destination_host.as_deref().unwrap_or("-");
    let secret = event.secret_name.as_deref().unwrap_or("-");
    let status = event
        .response_status
        .map(|status| status.to_string())
        .unwrap_or_else(|| "-".to_owned());
    let duration = event
        .duration_ms
        .map(|duration| format!("{duration}ms"))
        .unwrap_or_else(|| "-".to_owned());
    let request_bytes = event
        .request_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "-".to_owned());
    let response_bytes = event
        .response_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "-".to_owned());
    println!(
        "{}  id={} profile={} action={} host={} secret={} status={} duration={} request_bytes={} response_bytes={}",
        event.timestamp, event.id, event.profile, event.action, host, secret, status, duration,
        request_bytes, response_bytes
    );
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

const REMOTE_SESSION_ROTATE_EARLY: Duration = Duration::from_secs(120);
const REMOTE_SESSION_ROTATION_RETRY: Duration = Duration::from_secs(15);

fn remote_session_state(
    session: &crate::api::remote_proxy::RemoteProxySession,
) -> anyhow::Result<crate::handlers::run::proxy::RemoteProxySessionState> {
    let expires_at = DateTime::parse_from_rfc3339(&session.expires_at)
        .map_err(|error| {
            anyhow::anyhow!("Agent Proxy returned an invalid session expiry: {error}")
        })?
        .with_timezone(&Utc);
    Ok(crate::handlers::run::proxy::RemoteProxySessionState {
        token: session.session_token.clone(),
        expires_at,
        last_rotation_error: None,
    })
}

/// Forward-proxy sessions need the public interception CA before the child is
/// spawned. Custom-header sessions do not use a TLS-intercepting proxy.
fn provision_remote_session_ca(
    session: &crate::api::remote_proxy::RemoteProxySession,
) -> anyhow::Result<Option<std::path::PathBuf>> {
    if session.protocol == "http/1.1-forward-proxy-tls-intercept" {
        let certificate = session
            .proxy_ca
            .as_ref()
            .context("Agent Proxy session did not include its TLS interception CA")?;
        return crate::handlers::run::proxy::provision_remote_proxy_ca(certificate).map(Some);
    }
    Ok(None)
}

/// Child processes commonly load their TLS trust roots once at startup. A
/// replacement session must therefore keep the same transport and interception
/// CA as the initial session; accepting a different CA would leave the child
/// and local relay trusting the stale file.
fn remote_session_transport_identity(
    session: &crate::api::remote_proxy::RemoteProxySession,
) -> anyhow::Result<(String, Option<(String, String)>)> {
    let ca = if session.protocol == "http/1.1-forward-proxy-tls-intercept" {
        let certificate = session
            .proxy_ca
            .as_ref()
            .context("Agent Proxy session did not include its TLS interception CA")?;
        Some((certificate.key_id.clone(), certificate.sha256.clone()))
    } else {
        None
    };
    Ok((session.protocol.clone(), ca))
}

fn ensure_replacement_session_is_compatible(
    initial_transport: &(String, Option<(String, String)>),
    replacement: &crate::api::remote_proxy::RemoteProxySession,
) -> anyhow::Result<()> {
    let replacement_transport = remote_session_transport_identity(replacement)?;
    if initial_transport.0 != replacement_transport.0 {
        anyhow::bail!(
            "Agent Proxy changed protocols while rotating a session; restart the agent run"
        );
    }
    if initial_transport.1 != replacement_transport.1 {
        anyhow::bail!(
            "Agent Proxy changed its TLS interception CA while rotating a session; restart the agent run"
        );
    }
    Ok(())
}

fn remote_session_rotation_delay(expires_at: DateTime<Utc>) -> Duration {
    let remaining = (expires_at - Utc::now()).to_std().unwrap_or_default();
    remote_session_rotation_delay_for(remaining)
}

fn remote_session_rotation_delay_for(remaining: Duration) -> Duration {
    // A normal ten-minute session rotates two minutes early. For deliberately
    // short test sessions, keep a proportional grace period instead of issuing
    // a replacement every second.
    let lead_time = REMOTE_SESSION_ROTATE_EARLY
        .min(remaining / 5)
        .max(Duration::from_secs(1));
    remaining
        .saturating_sub(lead_time)
        .max(Duration::from_secs(1))
}

/// Rotates the control-plane session before it expires. It deliberately never
/// retries child requests: only future connections see the replacement token.
fn spawn_remote_session_rotation(
    request: crate::api::remote_proxy::RemoteProxySessionRequest,
    initial_session: crate::api::remote_proxy::RemoteProxySession,
    state: Arc<RwLock<crate::handlers::run::proxy::RemoteProxySessionState>>,
    initial_transport: (String, Option<(String, String)>),
) -> (watch::Sender<bool>, JoinHandle<()>) {
    let (stop, mut stop_rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        let mut session = initial_session;
        let mut retry_pending = false;
        loop {
            let expires_at = match remote_session_state(&session) {
                Ok(state) => state.expires_at,
                Err(error) => {
                    if let Ok(mut current) = state.write() {
                        current.last_rotation_error = Some(error.to_string());
                    }
                    return;
                }
            };
            if !retry_pending {
                tokio::select! {
                    _ = tokio::time::sleep(remote_session_rotation_delay(expires_at)) => {}
                    result = stop_rx.changed() => {
                        if result.is_ok() && *stop_rx.borrow() { return; }
                        return;
                    }
                }
            }
            retry_pending = false;

            let previous_session_token = match state.read() {
                Ok(current) => current.token.clone(),
                Err(_) => return,
            };
            let replacement_request = request.replacement(previous_session_token);
            match crate::api::remote_proxy::create_session(&replacement_request, false).await {
                Ok(next_session) => {
                    let next_state = match ensure_replacement_session_is_compatible(
                        &initial_transport,
                        &next_session,
                    )
                    .and_then(|_| provision_remote_session_ca(&next_session))
                    .and_then(|_| remote_session_state(&next_session))
                    {
                        Ok(next_state) => next_state,
                        Err(error) => {
                            crate::api::remote_proxy::revoke_session(
                                request.api_key.clone(),
                                &next_session.session_token,
                            )
                            .await;
                            if let Ok(mut current) = state.write() {
                                current.last_rotation_error = Some(error.to_string());
                            }
                            // Keep the current session active until its stated expiry.
                            // A malformed replacement response is recoverable; retrying
                            // later avoids permanently disabling rotation for this run.
                            let retry = REMOTE_SESSION_ROTATION_RETRY
                                .min((expires_at - Utc::now()).to_std().unwrap_or_default());
                            if retry.is_zero() {
                                return;
                            }
                            tokio::select! {
                                _ = tokio::time::sleep(retry) => {}
                                _ = stop_rx.changed() => return,
                            }
                            retry_pending = true;
                            continue;
                        }
                    };
                    let old_token = {
                        let mut current = match state.write() {
                            Ok(current) => current,
                            Err(_) => return,
                        };
                        let old_token = current.token.clone();
                        *current = next_state;
                        // Update the signal-handler cleanup token while holding
                        // the write lock. This closes the TOCTOU window where a
                        // signal arriving between the state write and a separate
                        // cleanup update would send DELETE with the replaced
                        // (old) token instead of the newly issued one.
                        crate::api::remote_proxy::update_agent_run_cleanup_token(
                            next_session.session_token.clone(),
                        );
                        old_token
                    };

                    // The control plane retains the replaced token for its
                    // server-side grace window, then rejects new handshakes.
                    crate::api::remote_proxy::retire_session(request.api_key.clone(), &old_token)
                        .await;
                    session = next_session;
                }
                Err(error) => {
                    if let Ok(mut current) = state.write() {
                        current.last_rotation_error = Some(error.to_string());
                    }
                    // Keep the active session usable until its stated expiry. A
                    // later retry may succeed without disrupting open streams.
                    let retry = REMOTE_SESSION_ROTATION_RETRY
                        .min((expires_at - Utc::now()).to_std().unwrap_or_default());
                    if retry.is_zero() {
                        return;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(retry) => {}
                        _ = stop_rx.changed() => return,
                    }
                    retry_pending = true;
                }
            }
        }
    });
    (stop, task)
}

#[cfg(test)]
mod tests {
    use super::{
        configured_host_matches, directory_profile_git_warning,
        ensure_replacement_session_is_compatible, infer_remote_agent_type,
        remote_session_rotation_delay_for, remote_session_transport_identity, secret_child_name,
        summarize_audit_events,
    };
    use crate::handlers::run::proxy::ProxyAuditLogEvent;
    use crate::models::agent::AgentSecretProfile;
    use std::{
        fs,
        path::Path,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn temporary_git_repository() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "stashbase-agent-profile-git-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        git2::Repository::init(&directory).unwrap();
        directory
    }

    #[test]
    fn local_secret_binding_uses_the_configured_child_environment_name() {
        let secret = AgentSecretProfile {
            hosts: Vec::new(),
            rules: Vec::new(),
            from: None,
            env: Some("GH_TOKEN".to_owned()),
            placeholder: None,
            header: None,
            value_template: None,
        };

        assert_eq!(
            secret_child_name("GITHUB_TOKEN", &secret, false),
            "GH_TOKEN"
        );
        assert_eq!(
            secret_child_name("GITHUB_TOKEN", &secret, true),
            "GITHUB_TOKEN"
        );
    }

    #[test]
    fn warns_only_for_modified_tracked_directory_profiles() {
        let directory = temporary_git_repository();
        let profile = directory.join(".stashbase/agents/codex.toml");
        fs::create_dir_all(profile.parent().unwrap()).unwrap();
        fs::write(&profile, "egress_hosts = [\"api.github.com\"]\n").unwrap();

        assert!(
            directory_profile_git_warning(&profile, "./.stashbase/agents/codex.toml").is_none()
        );

        let repository = git2::Repository::open(&directory).unwrap();
        let mut index = repository.index().unwrap();
        index
            .add_path(Path::new(".stashbase/agents/codex.toml"))
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Stashbase test", "test@example.com").unwrap();
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "add profile",
                &tree,
                &[],
            )
            .unwrap();
        fs::write(
            &profile,
            "egress_hosts = [\"api.github.com\", \"chatgpt.com\"]\n",
        )
        .unwrap();

        assert!(
            directory_profile_git_warning(&profile, "./.stashbase/agents/codex.toml")
                .unwrap()
                .contains("uncommitted")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn configured_host_matching_supports_exact_and_subdomain_wildcards() {
        assert!(configured_host_matches(
            "api.stashbase.dev",
            "api.stashbase.dev"
        ));
        assert!(configured_host_matches(
            "*.stashbase.dev",
            "api.stashbase.dev"
        ));
        assert!(!configured_host_matches("*.stashbase.dev", "stashbase.dev"));
        assert!(!configured_host_matches(
            "api.stashbase.dev",
            "other.example"
        ));
    }

    #[test]
    fn audit_summary_groups_denials_without_secret_metadata() {
        let event = |action: &str, host: &str, status| ProxyAuditLogEvent {
            timestamp: "2026-01-01T00:00:00Z".to_owned(),
            id: "evt_test".to_owned(),
            session_id: "session".to_owned(),
            profile: "codex".to_owned(),
            policy_fingerprint: "fingerprint".to_owned(),
            profile_source: None,
            profile_file_modified_at: None,
            profile_file_sha256: None,
            action: action.to_owned(),
            destination_host: Some(host.to_owned()),
            method: Some("GET".to_owned()),
            secret_name: Some("GITHUB_TOKEN".to_owned()),
            response_status: status,
            duration_ms: Some(1),
            request_bytes: Some(10),
            response_bytes: Some(20),
        };
        let report = summarize_audit_events(
            vec![
                event("injected", "api.github.com", Some(200)),
                event("credential_rule_denied", "api.github.com", Some(403)),
                event("credential_rule_denied", "api.github.com", Some(403)),
                event("host_denied", "api.stripe.com", Some(403)),
            ],
            50,
            Some("7d".to_owned()),
            None,
        );

        assert_eq!(report.limit, 50);
        assert_eq!(report.since.as_deref(), Some("7d"));
        assert_eq!(report.events, 4);
        assert_eq!(report.requests, 4);
        assert_eq!(report.injected, 1);
        assert_eq!(report.denied, 3);
        assert_eq!(report.request_bytes, 40);
        assert_eq!(report.response_bytes, 80);
        assert_eq!(report.denied_by[0].action, "credential_rule_denied");
        assert_eq!(report.denied_by[0].host, "api.github.com");
        assert_eq!(report.denied_by[0].count, 2);
    }

    #[test]
    fn audit_summary_groups_transfer_totals_by_requested_dimension() {
        let events = vec![
            ProxyAuditLogEvent {
                timestamp: "2026-01-01T00:00:00Z".to_owned(),
                id: "evt_one".to_owned(),
                session_id: "session".to_owned(),
                profile: "codex".to_owned(),
                policy_fingerprint: "fingerprint".to_owned(),
                profile_source: None,
                profile_file_modified_at: None,
                profile_file_sha256: None,
                action: "injected".to_owned(),
                destination_host: Some("api.github.com".to_owned()),
                method: Some("POST".to_owned()),
                secret_name: Some("GITHUB_TOKEN".to_owned()),
                response_status: Some(200),
                duration_ms: Some(1),
                request_bytes: Some(10),
                response_bytes: Some(100),
            },
            ProxyAuditLogEvent {
                timestamp: "2026-01-01T00:00:00Z".to_owned(),
                id: "evt_two".to_owned(),
                session_id: "session".to_owned(),
                profile: "codex".to_owned(),
                policy_fingerprint: "fingerprint".to_owned(),
                profile_source: None,
                profile_file_modified_at: None,
                profile_file_sha256: None,
                action: "forwarded".to_owned(),
                destination_host: Some("registry.npmjs.org".to_owned()),
                method: Some("GET".to_owned()),
                secret_name: None,
                response_status: Some(200),
                duration_ms: Some(1),
                request_bytes: Some(2),
                response_bytes: Some(200),
            },
        ];

        let by_host =
            super::summarize_audit_groups(&events, crate::cmd::agent::AgentAuditGroupBy::Host);
        assert_eq!(by_host[0].value, "registry.npmjs.org");
        assert_eq!(by_host[0].response_bytes, 200);

        let by_action =
            super::summarize_audit_groups(&events, crate::cmd::agent::AgentAuditGroupBy::Action);
        assert_eq!(by_action[1].value, "injected");
        assert_eq!(by_action[1].request_bytes, 10);

        let by_secret =
            super::summarize_audit_groups(&events, crate::cmd::agent::AgentAuditGroupBy::Secret);
        assert_eq!(by_secret[0].value, "-");
        assert_eq!(by_secret[1].value, "GITHUB_TOKEN");
    }

    #[test]
    fn denied_summary_table_uses_the_shared_table_renderer() {
        let entries = vec![
            super::AuditDeniedSummary {
                action: "host_denied".to_owned(),
                host: "api.stripe.com".to_owned(),
                count: 5,
            },
            super::AuditDeniedSummary {
                action: "credential_rule_denied".to_owned(),
                host: "api.github.com".to_owned(),
                count: 12,
            },
        ];
        let output = super::format_denied_summary_table(&entries);
        assert!(output.contains("ACTION"));
        assert!(output.contains("credential_rule_denied"));
        assert!(output.contains("api.stripe.com"));
    }

    #[test]
    fn formats_byte_counts_for_human_audit_output() {
        assert_eq!(super::format_bytes(0), "0 B");
        assert_eq!(super::format_bytes(1_023), "1023 B");
        assert_eq!(super::format_bytes(1_024), "1.00 KiB");
        assert_eq!(super::format_bytes(12 * 1_024), "12.0 KiB");
        assert_eq!(super::format_bytes(128 * 1_024), "128 KiB");
        assert_eq!(super::format_bytes(1_572_864), "1.50 MiB");
        assert_eq!(super::format_bytes(3 * 1_024 * 1_024 * 1_024), "3.00 GiB");
    }

    #[test]
    fn short_remote_sessions_rotate_proportionally_instead_of_every_second() {
        assert_eq!(
            remote_session_rotation_delay_for(Duration::from_secs(10)),
            Duration::from_secs(8)
        );
        assert_eq!(
            remote_session_rotation_delay_for(Duration::from_secs(600)),
            Duration::from_secs(480)
        );
    }

    #[test]
    fn remote_agent_type_uses_only_the_executable_basename() {
        let classify = |value: &str| infer_remote_agent_type(&[value.to_owned()]);

        assert_eq!(classify("/usr/local/bin/codex"), "codex");
        assert_eq!(classify("codex.exe"), "codex");
        assert_eq!(classify("claude-code"), "claude-code");
        assert_eq!(classify("github-copilot"), "copilot");
        assert_eq!(classify("cursor-agent"), "cursor");
        assert_eq!(classify("opencode"), "opencode");
        assert_eq!(classify("my-wrapper"), "custom");
        assert_eq!(infer_remote_agent_type(&[]), "custom");
    }

    #[test]
    fn replacement_session_rejects_a_changed_interception_ca() {
        let initial = crate::api::remote_proxy::RemoteProxySession {
            session_id: "session-1".to_owned(),
            session_token: "token-1".to_owned(),
            expires_at: "2026-01-01T00:00:00Z".to_owned(),
            proxy_url: "https://proxy.example".to_owned(),
            protocol: "http/1.1-forward-proxy-tls-intercept".to_owned(),
            proxy_ca: Some(crate::api::remote_proxy::RemoteProxyCa {
                key_id: "ca-1".to_owned(),
                sha256: "first".to_owned(),
                pem: "unused".to_owned(),
            }),
        };
        let replacement = crate::api::remote_proxy::RemoteProxySession {
            session_id: "session-2".to_owned(),
            session_token: "token-2".to_owned(),
            expires_at: "2026-01-01T00:10:00Z".to_owned(),
            proxy_url: "https://proxy.example".to_owned(),
            protocol: "http/1.1-forward-proxy-tls-intercept".to_owned(),
            proxy_ca: Some(crate::api::remote_proxy::RemoteProxyCa {
                key_id: "ca-2".to_owned(),
                sha256: "second".to_owned(),
                pem: "unused".to_owned(),
            }),
        };

        let identity = remote_session_transport_identity(&initial).unwrap();
        let error = ensure_replacement_session_is_compatible(&identity, &replacement).unwrap_err();
        assert!(error.to_string().contains("TLS interception CA"));
    }
}
