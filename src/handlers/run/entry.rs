use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context};
use log::debug;
use spinoff::{Spinner, Streams};
use tabled::Tabled;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    handlers::run::subprocess,
    models::{
        api_client::{GetRequestApiResponse, OutputError},
        config_env::{ConfigActionCommand, EnvConfigItem},
        scope::Scope,
        secrets::{PrintSecrets, SecretOnlyName, SecretWithoutComment},
        validation::{
            InputValidationError, LoadEnvironmentInputValidationError, RunInputValidationError,
            SecretsInputValidationError,
        },
    },
    utils::{
        env,
        interaction::{self},
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
        secrets::read_secrets_from_file,
        separator,
        tables::build::build_table,
        validation::{
            map_secret_to_load_exclude_secrets_error, map_secret_to_load_only_secrets_error,
            map_secret_to_load_set_secrets_error, validate_project_environment_identifier,
            validate_secret_names,
        },
    },
    SUBPROCESS_RUNNING,
};

use super::format::format_env_variable_value;

/// Runs an agent through the localhost relay while credentials stay in the
/// control-plane's short-lived remote agent-proxy session.
pub async fn handle_remote_agent_run(
    command: Vec<String>,
    policy: super::proxy::ProxyPolicy,
    remote: super::proxy::RemoteProxyConfig,
    proxy_port: Option<u16>,
    sandbox: bool,
    trust_proxy_ca: bool,
    audit_log: Option<super::proxy::ProxyAuditLog>,
    source_env_names: Vec<String>,
    silent: bool,
) -> anyhow::Result<()> {
    let cmd = command.first().context("no command provided")?.clone();
    let args = command.into_iter().skip(1).collect();
    let denied_commands = policy.denied_commands.iter().cloned().collect::<Vec<_>>();
    let argument_denied_commands = policy.argument_denied_commands.clone();
    let denied_read_paths = policy.denied_read_paths.clone();
    let denied_write_paths = policy.denied_write_paths.clone();
    let command_audit_log = audit_log.clone();
    let proxy =
        super::proxy::Proxy::start_remote_with_port(remote, policy, audit_log, proxy_port).await?;
    let _trusted_ca = trust_proxy_ca.then(|| proxy.trust_ca()).transpose()?;
    if !silent {
        let address = proxy.child_env()["HTTP_PROXY"].trim_start_matches("http://");
        eprintln!(
            "Remote agent proxy relay started on localhost:{}",
            address.rsplit(':').next().unwrap_or_default()
        );
        eprintln!("Remote agent proxy session active");
    }
    let result = subprocess::run_command_with_denied_commands(
        &cmd,
        args,
        proxy.child_env().clone(),
        source_env_names,
        sandbox,
        true,
        true,
        &denied_commands,
        &argument_denied_commands,
        &denied_read_paths,
        &denied_write_paths,
        command_audit_log,
    )
    .await;
    proxy.stop().await;
    if !silent {
        eprintln!("Remote agent proxy relay stopped");
    }
    let status = result?;
    if !status.success() {
        return Err(subprocess::CommandFailed { status }.into());
    }
    Ok(())
}

#[derive(Debug)]
pub struct HandleRunArgs {
    pub api_key: String,
    pub project: Option<String>,
    pub environment: Option<String>,
    pub command: Vec<String>,
    pub proxy: bool,
    pub proxy_port: Option<u16>,
    pub proxy_policy: Option<super::proxy::ProxyPolicy>,
    pub trust_proxy_ca: bool,
    pub sandbox: bool,
    pub audit_log: Option<super::proxy::ProxyAuditLog>,
    /// Maps fetched source secret names to the names exposed to the child.
    pub secret_bindings: HashMap<String, String>,
    /// Allows a profile file to override values fetched from project/environment.
    pub allow_file_override: bool,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
    pub set: Vec<String>,
    pub set_comments: Vec<String>,
    pub print_secrets: Option<PrintSecrets>,
    pub no_print_secrets: bool,
    pub config_file: Option<String>,
    pub file: Option<String>,
    pub expand_refs: Option<bool>,
    pub json_format: bool,
    pub silent: bool,
    pub scope: Option<Scope>,
}

pub async fn handle_load_env_run(args: HandleRunArgs) -> anyhow::Result<()> {
    let HandleRunArgs {
        api_key,
        command,
        proxy,
        proxy_port,
        proxy_policy,
        trust_proxy_ca,
        sandbox,
        audit_log,
        secret_bindings,
        allow_file_override,
        config_file,
        file,
        mut set,
        set_comments,
        mut project,
        mut environment,
        mut only,
        mut exclude,
        mut expand_refs,
        mut print_secrets,
        no_print_secrets,
        json_format,
        silent,
        scope,
    } = args;

    if no_print_secrets {
        print_secrets = None;
    }

    // Handle environment scope - workspace scope behaves like no scope
    let is_environment_scope = scope.as_ref() == Some(&Scope::Environment);

    if file.is_some() && (project.is_some() || environment.is_some()) && !allow_file_override {
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::FileArgWithInline,
        );
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    if command.is_empty() {
        let error = InputValidationError::Run(RunInputValidationError::NoCmdProvided);
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    // An agent profile with no source and no secret bindings is intentionally
    // egress-only. Do not fall through to normal `run` config discovery: that
    // could load unrelated repository secrets into a no-secret agent session.
    let egress_only = proxy_policy
        .as_ref()
        .is_some_and(|policy| policy.strict_deny)
        && secret_bindings.is_empty()
        && file.is_none()
        && project.is_none()
        && environment.is_none();
    if egress_only {
        let mut spinner = None;
        return handle_run(
            &mut spinner,
            command,
            proxy,
            proxy_port,
            proxy_policy,
            trust_proxy_ca,
            sandbox,
            audit_log,
            &secret_bindings,
            None,
            Vec::new(),
            false,
            silent,
            json_format,
        )
        .await;
    }

    if let Err(error) = validate_no_duplicate_set_names(&set) {
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    let mut is_from_file = false;
    let mut file_secrets: Option<Vec<SecretWithoutComment>> = None;

    let mut setted_secrets = HashMap::<String, String>::new();

    if let Some(input_path) = &file {
        file_secrets = Some(load_run_secrets_from_file(input_path, json_format, silent)?);
    }

    if is_environment_scope {
        // For environment scope, load from API (not from file)
        is_from_file = false;
    } else if let (Some(_), Some(_)) = (&project, &environment) {
        is_from_file = false;
    } else if let Some(_) = project {
        // missing env arg
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::MissingEnvArg,
        );
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    } else if let Some(_) = environment {
        // missing project error
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::MissingProjectArg,
        );
        let formatted_err = error.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    } else if file.is_some() {
        is_from_file = true;
    } else {
        let config_action_command = ConfigActionCommand::Run;
        // LOAD from file
        let selected_config_item =
            EnvConfigItem::select_from_file(config_file, &config_action_command)?;

        if let Some(config) = selected_config_item {
            let secrets_config = config.get_run_secrets();

            // expand refs
            if let Some(expand_refs_val) = secrets_config.expand_refs {
                if expand_refs.is_none() {
                    expand_refs = Some(expand_refs_val);
                }
            }

            // print
            if !no_print_secrets {
                if let Some(print_secrets_val) = secrets_config.print.clone() {
                    if print_secrets.is_none() {
                        print_secrets = Some(print_secrets_val);
                    }
                }
            }

            // only
            if let Some(only_val) = secrets_config.only {
                if only_val.is_empty() == false {
                    for only_secret in only_val {
                        let already_exists = only.contains(&only_secret);

                        if !already_exists {
                            only.push(only_secret);
                        }
                    }
                }
            }

            // exclude
            if let Some(exclude_val) = secrets_config.exclude {
                if exclude_val.is_empty() == false {
                    for exclude_secret in exclude_val {
                        let already_exists = exclude.contains(&exclude_secret);

                        if !already_exists {
                            exclude.push(exclude_secret);
                        }
                    }
                }
            }

            // set
            if let Some(set_val) = secrets_config.set {
                if set_val.is_empty() == false {
                    let mut set_secrets_from_file = Vec::new();
                    let mut seen_set_names = HashSet::new();
                    let mut duplicate_set_names = Vec::new();
                    let mut duplicate_set_seen = HashSet::new();

                    for item in set_val {
                        if !seen_set_names.insert(item.name.clone())
                            && duplicate_set_seen.insert(item.name.clone())
                        {
                            duplicate_set_names.push(item.name.clone());
                        }

                        let name_value_str = format!("{}={}", item.name, item.value);

                        if set.contains(&name_value_str) == false {
                            set_secrets_from_file.push(name_value_str);
                        }
                    }

                    if !duplicate_set_names.is_empty() {
                        let error = InputValidationError::LoadEnvironment(
                            LoadEnvironmentInputValidationError::SetDuplicateNames(
                                duplicate_set_names,
                            ),
                        );
                        let formatted_err = error.format_error_output(json_format)?;

                        if !silent {
                            eprintln!();
                        }
                        bail!(formatted_err);
                    }

                    set = [set_secrets_from_file, set].concat();
                }
            }

            project = Some(config.project);
            environment = Some(config.environment);
        } else {
            if !silent {
                eprintln!("\nRun command exited");
            }
            return Ok(());
        }
    }

    // Only validate project/environment if not using environment scope
    if !is_environment_scope {
        if let (Some(ref proj), Some(ref env)) = (&project, &environment) {
            let validation_res = validate_project_environment_identifier(proj, env, true);

            if let Err(e) = validation_res {
                let formatted_err = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(formatted_err);
            }
        }
    }

    if !only.is_empty() && !exclude.is_empty() {
        let err = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::UseOfBothExcludeAndOnly,
        );

        let formatted_err = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }
        bail!(formatted_err);
    }

    if !only.is_empty() {
        let name_validation_res = validate_secret_names(&only);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_only_secrets_error(&err);
            let error = InputValidationError::LoadEnvironment(mapped_err);
            let formatted_err = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(formatted_err);
        }
    }

    if !exclude.is_empty() {
        let name_validation_res = validate_secret_names(&exclude);

        if let Err(err) = name_validation_res {
            let mapped_err = map_secret_to_load_exclude_secrets_error(&err);
            let error = InputValidationError::LoadEnvironment(mapped_err);
            let formatted_err = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(formatted_err);
        }
    }

    if !set.is_empty() {
        let name_values_pairs = get_set_name_value_pairs(set);

        match name_values_pairs {
            Ok(secrets) => {
                for (name, value) in secrets {
                    setted_secrets.insert(name, value);
                }
            }
            Err(e) => {
                let formatted_err = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(formatted_err);
            }
        }
    }

    if !set_comments.is_empty() {
        let comments_pairs = get_set_name_comment_pairs(set_comments);

        match comments_pairs {
            Ok(comments) => {
                let mut missing_set_names = Vec::<String>::new();

                for (name, _) in comments {
                    if !setted_secrets.contains_key(&name) {
                        missing_set_names.push(name);
                    }
                }

                if !missing_set_names.is_empty() {
                    let error = InputValidationError::LoadEnvironment(
                        LoadEnvironmentInputValidationError::SetCommentWithoutSet(
                            missing_set_names,
                        ),
                    );
                    let formatted_err = error.format_error_output(json_format)?;

                    if !silent {
                        eprintln!();
                    }
                    bail!(formatted_err);
                }
            }
            Err(e) => {
                let formatted_err = e.format_error_output(json_format)?;

                if !silent {
                    eprintln!();
                }
                bail!(formatted_err);
            }
        }
    }

    let setted_len = setted_secrets.len();

    if setted_len > 0 && setted_len == only.len() {
        let exists_count = setted_secrets
            .iter()
            .filter(|secret| only.contains(&secret.0))
            .count();

        if exists_count == setted_len {
            let run_error = RunInputValidationError::NoSecretsToFetch;
            let error = InputValidationError::Run(run_error);
            let formatted_err = error.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }
            bail!(formatted_err);
        }
    }

    // exclude manually
    if !setted_secrets.is_empty() {
        for secret in setted_secrets.iter() {
            let name = secret.0;

            let exists = exclude.contains(&name);

            // if !exists {
            //     exclude.push(key.to_string());
            // }

            if !exists && only.is_empty() {
                exclude.push(name.to_string());
            }

            let only_exists = only.contains(&name);
            if only_exists {
                // remove from only
                if let Some(index) = only.iter().position(|x| x == name) {
                    only.remove(index);
                }
            }
        }
    }

    let only_len = only.len();

    if is_from_file && !silent {
        eprintln!();
    }

    let mut spinner = if !silent {
        Some(crate::utils::spinner::new_spinner(
            "Loading environment...",
            Streams::Stderr,
        ))
    } else {
        None
    };

    // Determine project and environment for API call
    let (api_project, api_environment) = if is_environment_scope {
        // For environment scope, pass None (relies on environment-scoped API key)
        (None, None)
    } else {
        (project.clone(), environment.clone())
    };
    let local_overrides = (!is_from_file)
        .then(|| {
            file_secrets
                .as_ref()
                .map(|secrets| prepare_local_run_secrets(secrets.clone(), &only, &exclude))
        })
        .flatten();

    if is_from_file {
        let mut secrets =
            prepare_local_run_secrets(file_secrets.unwrap_or_default(), &only, &exclude);
        let missing_secrets = missing_secret_labels(&only, &secrets, &secret_bindings);

        if secrets.is_empty() && setted_secrets.is_empty() {
            let message = if only_len == 0 {
                "No secrets found.".to_string()
            } else {
                format!("{} secret(s) requested, no secrets found.", only_len)
            };

            if json_format {
                let message = serde_json::json!({
                    "error": {
                        "message": message,
                        "details": {
                            "missing_secrets": missing_secrets,
                        }
                    }
                });

                let json_str = get_formatted_json_string(&message, false).unwrap();
                eprintln!("{}", json_str);
            } else if let Some(ref mut spinner) = spinner {
                spinner.stop_with_message(&format!(
                    "{}\n  Message: {}\n  Details:\n    Missing secrets: {}",
                    "Error".red_if_tty_stderr(),
                    message,
                    missing_secrets.join(", ")
                ));
            } else if !silent {
                eprintln!(
                    "{}\n  Message: {}\n  Details:\n    Missing secrets: {}",
                    "Error".red_if_tty_stderr(),
                    message,
                    missing_secrets.join(", ")
                );
            }

            return Ok(());
        }

        if only_len > 0 && secrets.len() < only_len {
            let mut msg = format!(
                "{} {} Secret(s) found, {} secret(s) requested.",
                "Error:".red_if_tty_stderr(),
                secrets.len(),
                only_len
            );

            msg.insert_str(0, "\n");
            msg.push_str(&format!("\n  Missing: {}", missing_secrets.join(", ")));

            // `spinoff` spinners can only be stopped once. The confirmed run
            // below will finish its own spinner, so discard this one after
            // clearing the warning line.
            if let Some(mut spinner) = spinner.take() {
                spinner.stop_and_persist("", "");
            }

            if !silent {
                eprintln!("{}", msg);
            }

            let confirmation = if !silent {
                interaction::confirm_opt("Do you still want to proceed?")
            } else {
                Some(true)
            };

            if confirmation != Some(true) {
                return Ok(());
            }
        }

        if !setted_secrets.is_empty() {
            for (name, value) in setted_secrets {
                secrets.push(SecretWithoutComment { name, value });
            }
        }

        for secret in secrets.iter_mut() {
            secret.value = format_env_variable_value(secret.value.to_string());
        }

        handle_run(
            &mut spinner,
            command,
            proxy,
            proxy_port,
            proxy_policy.clone(),
            trust_proxy_ca,
            sandbox,
            audit_log.clone(),
            &secret_bindings,
            print_secrets.clone(),
            secrets,
            is_from_file,
            silent,
            json_format,
        )
        .await?;

        return Ok(());
    }

    let remote_only = local_overrides
        .as_ref()
        .map(|local_secrets| {
            let local_names = local_secrets
                .iter()
                .map(|secret| secret.name.as_str())
                .collect::<HashSet<_>>();
            only.iter()
                .filter(|name| !local_names.contains(name.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| only.clone());

    if remote_only.is_empty() {
        let mut secrets = local_overrides.unwrap_or_default();
        for secret in &mut secrets {
            secret.value = format_env_variable_value(secret.value.to_string());
        }
        handle_run(
            &mut spinner,
            command,
            proxy,
            proxy_port,
            proxy_policy.clone(),
            trust_proxy_ca,
            sandbox,
            audit_log.clone(),
            &secret_bindings,
            print_secrets.clone(),
            secrets,
            false,
            silent,
            json_format,
        )
        .await?;
        return Ok(());
    }

    if api_key.is_empty() {
        let error = InputValidationError::MissingApiKey;
        let formatted_err = error.format_error_output(json_format)?;
        if let Some(ref mut spinner) = spinner {
            spinner.stop_with_message(&formatted_err);
        } else if !silent {
            eprintln!("{formatted_err}");
        }
        return Ok(());
    }

    let res = secrets::pull(
        api_key,
        api_project,
        api_environment,
        remote_only,
        exclude,
        false,
        expand_refs.unwrap_or(false),
    )
    .await;

    if let Err(err) = res {
        debug!("Error: {:#?}", &err);
        let formatted_err = err.format_error_output(json_format)?;

        if let Some(mut spinner) = spinner {
            spinner.stop_with_message(&formatted_err);
        } else {
            eprintln!("{}", formatted_err);
        }

        return Ok(());
    }

    match res {
        Ok(GetRequestApiResponse::Ok(data)) => {
            // handle_ok_response(&mut spinner, command, only_len, print_secrets, data).await?;

            let secrets = serde_json::from_str::<Vec<SecretWithoutComment>>(&data.text);

            if let Ok(mut secrets) = secrets {
                if let Some(local_secrets) = local_overrides {
                    secrets = merge_remote_and_local_secrets(secrets, local_secrets, &only);
                }
                let missing_secrets = missing_secret_labels(&only, &secrets, &secret_bindings);
                if secrets.is_empty() && setted_secrets.is_empty() {
                    let message = if only_len == 0 {
                        "No secrets found.".to_owned()
                    } else {
                        format!("{} secret(s) requested, no secrets found.", only_len)
                    };
                    if json_format {
                        let message = serde_json::json!({
                            "error": {
                                "message": message,
                                "details": {
                                    "missing_secrets": missing_secrets,
                                }
                            }
                        });

                        let json_str = get_formatted_json_string(&message, false).unwrap();
                        eprintln!("{}", json_str);
                    } else {
                        let msg = format!(
                            "{}\n  Message: {}\n  Details:\n    Missing secrets: {}",
                            "Error".red_if_tty_stderr(),
                            message,
                            missing_secrets.join(", ")
                        );

                        if let Some(ref mut spinner) = spinner {
                            spinner.stop_with_message(&msg);
                        } else if !silent {
                            eprintln!("{}", msg);
                        }
                    }

                    return Ok(());
                }

                if only_len > 0 && secrets.len() < only_len {
                    let mut msg = format!(
                        "{} {} Secret(s) found, {} secret(s) requested.",
                        "Error:".red_if_tty_stderr(),
                        secrets.len(),
                        only_len
                    );

                    if !is_from_file {
                        msg.insert_str(0, "\n");
                    }
                    msg.push_str(&format!("\n  Missing: {}", missing_secrets.join(", ")));

                    // The warning ends the loading spinner before prompting.
                    // Do not leave a stopped spinner for `handle_run` to stop.
                    if let Some(mut spinner) = spinner.take() {
                        spinner.stop_and_persist("", "");
                    }

                    if !silent {
                        eprintln!("{}", msg);
                    }

                    let confirmation = if !silent {
                        interaction::confirm_opt("Do you still want to proceed?")
                    } else {
                        Some(true) // Auto-proceed in silent mode
                    };

                    if let Some(true) = confirmation {
                        if print_secrets.is_some() {
                            eprintln!();
                        }
                        // if !print_secrets {
                        //     eprintln!();
                        // }

                        if !setted_secrets.is_empty() {
                            for (name, value) in setted_secrets {
                                // secrets.push(SecretWithoutDescription { key, value })
                                secrets.push(SecretWithoutComment { name, value });
                            }
                        }

                        // format secret values (remove quotes if needed)
                        for s in secrets.iter_mut() {
                            s.value = format_env_variable_value(s.value.to_string());
                        }

                        handle_run(
                            &mut spinner,
                            command,
                            proxy,
                            proxy_port,
                            proxy_policy.clone(),
                            trust_proxy_ca,
                            sandbox,
                            audit_log.clone(),
                            &secret_bindings,
                            print_secrets.clone(),
                            secrets,
                            is_from_file,
                            silent,
                            json_format,
                        )
                        .await?;
                    } else {
                        return Ok(());
                    }
                } else {
                    if !setted_secrets.is_empty() {
                        for (name, value) in setted_secrets {
                            secrets.push(SecretWithoutComment { name, value });
                        }
                    }

                    // format secret values (remove quotes)
                    for s in secrets.iter_mut() {
                        s.value = format_env_variable_value(s.value.to_string());
                    }

                    handle_run(
                        &mut spinner,
                        command,
                        proxy,
                        proxy_port,
                        proxy_policy.clone(),
                        trust_proxy_ca,
                        sandbox,
                        audit_log.clone(),
                        &secret_bindings,
                        print_secrets.clone(),
                        secrets,
                        is_from_file,
                        silent,
                        json_format,
                    )
                    .await?;
                }
            } else {
                if let Some(ref mut spinner) = spinner {
                    spinner.stop_and_persist("", "");
                }

                let error = OutputError::failed_to_deserialize_response_body();
                let formatted_err = error.format_error_output(json_format)?;

                bail!(formatted_err);
            }
        }
        Ok(GetRequestApiResponse::Err(e)) => {
            if let Some(ref mut spinner) = spinner {
                spinner.stop_and_persist("", "");
            }
            bail!(e);
        }
        Err(_) => unreachable!(),
    }
    //
    Ok(())
}

fn prepare_local_run_secrets(
    mut secrets: Vec<SecretWithoutComment>,
    only: &[String],
    exclude: &[String],
) -> Vec<SecretWithoutComment> {
    if !only.is_empty() {
        secrets.retain(|secret| only.contains(&secret.name));
    }

    if !exclude.is_empty() {
        secrets.retain(|secret| !exclude.contains(&secret.name));
    }

    secrets
}

fn load_run_secrets_from_file(
    input_path: &str,
    json_format: bool,
    silent: bool,
) -> anyhow::Result<Vec<SecretWithoutComment>> {
    let path = Path::new(input_path);

    if !path.exists() {
        let err = InputValidationError::Secrets(SecretsInputValidationError::FileNotFound);
        let error_output = err.format_error_output(json_format)?;

        if !silent {
            eprintln!();
        }

        bail!(error_output);
    }

    let target_format = if input_path.ends_with(".yaml") || input_path.ends_with(".yml") {
        SecretsFileFormat::Yaml
    } else if input_path.ends_with(".json") {
        SecretsFileFormat::Json
    } else {
        SecretsFileFormat::Dotenv
    };

    let secrets_res = read_secrets_from_file(path, &target_format);

    let secrets = match secrets_res {
        Ok(secrets) => secrets,
        Err(err) => {
            let err = InputValidationError::Secrets(SecretsInputValidationError::ReadFile(
                err.to_string(),
            ));
            let error_output = err.format_error_output(json_format)?;

            if !silent {
                eprintln!();
            }

            bail!(error_output);
        }
    };

    Ok(secrets
        .into_iter()
        .map(|secret| SecretWithoutComment {
            name: secret.name,
            value: secret.value,
        })
        .collect())
}

async fn handle_run(
    spinner: &mut Option<Spinner>,
    command: Vec<String>,
    proxy: bool,
    proxy_port: Option<u16>,
    proxy_policy: Option<super::proxy::ProxyPolicy>,
    trust_proxy_ca: bool,
    sandbox: bool,
    audit_log: Option<super::proxy::ProxyAuditLog>,
    secret_bindings: &HashMap<String, String>,
    print_secrets: Option<PrintSecrets>,
    mut secrets: Vec<SecretWithoutComment>,
    is_from_file: bool,
    silent: bool,
    json_format: bool,
) -> anyhow::Result<()> {
    apply_secret_bindings(&mut secrets, secret_bindings);
    let secrets_hash_map = env::expand_and_inject_env(&mut secrets);

    if !silent {
        let mut success_msg = format!(
            "{} {} ({} {})",
            "✓".green_if_tty_stderr(),
            if secrets.is_empty() {
                "Egress-only profile"
            } else {
                "Environment loaded"
            },
            secrets.len(),
            if secrets.len() == 1 {
                "secret"
            } else {
                "secrets"
            }
        );

        if print_secrets.is_some() && !is_from_file {
            success_msg.insert_str(0, "\n");
            if let Some(mut spinner) = spinner.take() {
                spinner.stop_with_message(&success_msg);
            } else {
                eprintln!("{}", success_msg);
            }
        } else {
            if let Some(mut spinner) = spinner.take() {
                spinner.stop_with_message(&success_msg);
            } else {
                eprintln!("{}", success_msg);
            }
        }
    } else if let Some(mut spinner) = spinner.take() {
        spinner.stop_and_persist("", "");
    }
    // success msg

    if print_secrets.is_some() && !silent {
        let print_masked = print_secrets
            .as_ref()
            .map(|p| p.is_masked())
            .unwrap_or(false);

        if print_masked {
            let formatted_secrets: Vec<SecretWithoutComment> = secrets
                .clone()
                .into_iter()
                .map(|s| SecretWithoutComment {
                    name: s.name,
                    value: if s.value.len() <= 3 {
                        "*".repeat(6)
                    } else {
                        format!("{}{}", &s.value[..3], "*".repeat(6))
                    },
                })
                .collect();

            if json_format {
                let json_str = get_formatted_json_string(&formatted_secrets, true).unwrap();
                println!("{}\n", json_str);
            } else {
                print_table(&formatted_secrets);
            }
        } else if print_secrets.is_some_and(|p| p.is_name()) {
            // print only names
            let formatted_secrets: Vec<SecretOnlyName> = secrets
                .clone()
                .into_iter()
                .map(|s| SecretOnlyName { name: s.name })
                .collect();

            if json_format {
                let json_str = get_formatted_json_string(&formatted_secrets, true).unwrap();
                println!("{}\n", json_str);
            } else {
                print_table(&formatted_secrets);
            }
        } else {
            // print full
            if json_format {
                let json_str = get_formatted_json_string(&secrets, true).unwrap();
                println!("{}\n", json_str);
            } else {
                print_table(&secrets);
            }
        }
    }

    let mut mutex = SUBPROCESS_RUNNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *mutex = true;
    drop(mutex);

    let Some(cmd) = command.first().cloned() else {
        let error = InputValidationError::Run(RunInputValidationError::NoCmdProvided);
        let formatted_err = error.format_error_output(json_format)?;
        bail!(formatted_err);
    };

    let args = command
        .into_iter()
        .skip(1)
        .map(|s| s)
        .collect::<Vec<String>>();
    let restrict_stashbase_credentials = proxy_policy
        .as_ref()
        .is_some_and(|policy| policy.strict_deny);
    let denied_commands = proxy_policy
        .as_ref()
        .map(|policy| policy.denied_commands.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let argument_denied_commands = proxy_policy
        .as_ref()
        .map(|policy| policy.argument_denied_commands.clone())
        .unwrap_or_default();
    let denied_read_paths = proxy_policy
        .as_ref()
        .map(|policy| policy.denied_read_paths.clone())
        .unwrap_or_default();
    let denied_write_paths = proxy_policy
        .as_ref()
        .map(|policy| policy.denied_write_paths.clone())
        .unwrap_or_default();

    // Proxy mode gives the child placeholders, never the loaded secret values.
    // The temporary proxy owns the placeholder-to-secret mapping until the command exits.
    let command_result = if proxy {
        let command_audit_log = audit_log.clone();
        let proxy = super::proxy::Proxy::start_with_port(
            secrets_hash_map,
            proxy_policy.unwrap_or_else(super::proxy::ProxyPolicy::permissive),
            audit_log,
            proxy_port,
        )
        .await?;
        let _trusted_ca = trust_proxy_ca.then(|| proxy.trust_ca()).transpose()?;
        if !silent {
            let address = proxy.child_env()["HTTP_PROXY"].trim_start_matches("http://");
            eprintln!(
                "Agent proxy started on localhost:{}",
                address.rsplit(':').next().unwrap_or_default()
            );
        }
        let result = subprocess::run_command_with_denied_commands(
            &cmd,
            args,
            proxy.child_env().clone(),
            secret_bindings.keys().cloned().collect(),
            sandbox,
            true,
            restrict_stashbase_credentials,
            &denied_commands,
            &argument_denied_commands,
            &denied_read_paths,
            &denied_write_paths,
            command_audit_log,
        )
        .await;
        proxy.stop().await;
        if !silent {
            eprintln!("Agent proxy stopped");
        }
        result
    } else {
        // TODO: errors: no such file or directory
        subprocess::run_command(
            &cmd,
            args,
            secrets_hash_map,
            Vec::new(),
            sandbox,
            false,
            false,
        )
        .await
    };

    let mut mutex = SUBPROCESS_RUNNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *mutex = false;
    drop(mutex);

    let status = command_result?;
    if !status.success() {
        return Err(subprocess::CommandFailed { status }.into());
    }

    Ok(())
}

fn apply_secret_bindings(
    secrets: &mut Vec<SecretWithoutComment>,
    bindings: &HashMap<String, String>,
) {
    if bindings.is_empty() {
        return;
    }

    // A proxied profile should never expose an API response that was not one of
    // its explicitly requested source names.
    secrets.retain(|secret| bindings.contains_key(&secret.name));
    for secret in secrets {
        if let Some(target) = bindings.get(&secret.name) {
            secret.name.clone_from(target);
        }
    }
}

/// Combines the remote fallback with local overrides. Names outside the
/// profile's requested source set are discarded before a child can receive them.
fn merge_remote_and_local_secrets(
    remote: Vec<SecretWithoutComment>,
    local: Vec<SecretWithoutComment>,
    requested: &[String],
) -> Vec<SecretWithoutComment> {
    let mut merged = remote
        .into_iter()
        .filter(|secret| requested.contains(&secret.name))
        .map(|secret| (secret.name.clone(), secret))
        .collect::<HashMap<_, _>>();
    for secret in local {
        merged.insert(secret.name.clone(), secret);
    }
    merged.into_values().collect()
}

fn missing_secret_labels(
    requested: &[String],
    loaded: &[SecretWithoutComment],
    bindings: &HashMap<String, String>,
) -> Vec<String> {
    let loaded_names = loaded
        .iter()
        .map(|secret| secret.name.as_str())
        .collect::<HashSet<_>>();
    let mut missing = requested
        .iter()
        .filter(|source| !loaded_names.contains(source.as_str()))
        .map(|source| match bindings.get(source) {
            Some(target) if target != source => format!("{target} (from {source})"),
            _ => source.clone(),
        })
        .collect::<Vec<_>>();
    missing.sort();
    missing
}

#[cfg(test)]
mod tests {
    use super::{
        apply_secret_bindings, load_run_secrets_from_file, merge_remote_and_local_secrets,
        missing_secret_labels, prepare_local_run_secrets,
    };
    use crate::models::secrets::SecretWithoutComment;
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_file_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("stashbase-run-test-{}{}", nanos, suffix))
    }

    #[test]
    fn load_run_secrets_from_file_reads_dotenv_input() {
        let path = temp_file_path(".env");
        fs::write(&path, "FIRST=one\nSECOND=two\n").unwrap();

        let secrets = load_run_secrets_from_file(path.to_str().unwrap(), false, true).unwrap();

        fs::remove_file(&path).unwrap();

        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[0].name, "FIRST");
        assert_eq!(secrets[0].value, "one");
        assert_eq!(secrets[1].name, "SECOND");
        assert_eq!(secrets[1].value, "two");
    }

    #[test]
    fn load_run_secrets_from_file_reads_yaml_input() {
        let path = temp_file_path(".yaml");
        fs::write(&path, "FIRST: one\nSECOND: two\n").unwrap();

        let secrets = load_run_secrets_from_file(path.to_str().unwrap(), false, true).unwrap();

        fs::remove_file(&path).unwrap();

        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[0].name, "FIRST");
        assert_eq!(secrets[1].name, "SECOND");
    }

    #[test]
    fn prepare_local_run_secrets_applies_only_and_exclude_filters() {
        let secrets = vec![
            SecretWithoutComment {
                name: "FIRST".to_string(),
                value: "one".to_string(),
            },
            SecretWithoutComment {
                name: "SECOND".to_string(),
                value: "two".to_string(),
            },
            SecretWithoutComment {
                name: "THIRD".to_string(),
                value: "three".to_string(),
            },
        ];

        let filtered = prepare_local_run_secrets(
            secrets,
            &["FIRST".to_string(), "SECOND".to_string()],
            &["SECOND".to_string()],
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "FIRST");
    }

    #[test]
    fn secret_binding_renames_only_an_explicitly_requested_source() {
        let mut secrets = vec![
            SecretWithoutComment {
                name: "GITHUB_TOKEN".to_owned(),
                value: "token".to_owned(),
            },
            SecretWithoutComment {
                name: "UNREQUESTED".to_owned(),
                value: "must-not-reach-child".to_owned(),
            },
        ];

        apply_secret_bindings(
            &mut secrets,
            &HashMap::from([("GITHUB_TOKEN".to_owned(), "GH_TOKEN".to_owned())]),
        );

        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].name, "GH_TOKEN");
        assert_eq!(secrets[0].value, "token");
    }

    #[test]
    fn missing_secret_labels_include_the_child_binding_name() {
        let missing = missing_secret_labels(
            &["GITHUB_TOKEN".to_owned(), "OPENAI_API_KEY".to_owned()],
            &[SecretWithoutComment {
                name: "OPENAI_API_KEY".to_owned(),
                value: "token".to_owned(),
            }],
            &HashMap::from([("GITHUB_TOKEN".to_owned(), "GH_TOKEN".to_owned())]),
        );

        assert_eq!(missing, ["GH_TOKEN (from GITHUB_TOKEN)"]);
    }

    #[test]
    fn local_overrides_replace_remote_values_without_adding_unrequested_secrets() {
        let merged = merge_remote_and_local_secrets(
            vec![
                SecretWithoutComment {
                    name: "GITHUB_TOKEN".to_owned(),
                    value: "remote-token".to_owned(),
                },
                SecretWithoutComment {
                    name: "UNREQUESTED".to_owned(),
                    value: "must-not-reach-child".to_owned(),
                },
            ],
            vec![SecretWithoutComment {
                name: "GITHUB_TOKEN".to_owned(),
                value: "local-token".to_owned(),
            }],
            &["GITHUB_TOKEN".to_owned()],
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "GITHUB_TOKEN");
        assert_eq!(merged[0].value, "local-token");
    }
}

fn print_table(secrets: &Vec<impl Tabled>) {
    let table = build_table(secrets);
    println!("{}\n", table);
}

pub fn get_set_name_value_pairs(
    values: Vec<String>,
) -> Result<Vec<(String, String)>, InputValidationError> {
    let name_value_pairs_res = separator::key_value(values);

    match name_value_pairs_res {
        Ok(name_value_pairs) => {
            let names = name_value_pairs
                .iter()
                .map(|kv| format!("{}", kv.0))
                .collect::<Vec<String>>();
            // ok

            let names_validation = validate_secret_names(&names);

            match names_validation {
                Ok(_) => {
                    return Ok(name_value_pairs);
                }
                Err(err) => {
                    let mapped_err = map_secret_to_load_set_secrets_error(&err);
                    let error = InputValidationError::LoadEnvironment(mapped_err);

                    return Err(error);
                }
            }
        }
        Err(_) => {
            let error = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::SetSecretNameValueSeparator,
            );

            return Err(error);
        }
    }
}

pub fn validate_no_duplicate_set_names(values: &[String]) -> Result<(), InputValidationError> {
    let name_value_pairs = get_set_name_value_pairs(values.to_vec())?;
    let duplicate_names = get_duplicate_names(&name_value_pairs);

    if !duplicate_names.is_empty() {
        let error = InputValidationError::LoadEnvironment(
            LoadEnvironmentInputValidationError::SetDuplicateNames(duplicate_names),
        );
        return Err(error);
    }

    Ok(())
}

pub fn get_duplicate_names(pairs: &[(String, String)]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut duplicate_names = Vec::new();
    let mut duplicate_seen = HashSet::new();

    for (name, _) in pairs {
        if !seen.insert(name.clone()) && duplicate_seen.insert(name.clone()) {
            duplicate_names.push(name.clone());
        }
    }

    duplicate_names
}

pub fn get_set_name_comment_pairs(
    comments: Vec<String>,
) -> Result<Vec<(String, String)>, InputValidationError> {
    let name_comment_pairs_res = separator::key_value(comments);

    match name_comment_pairs_res {
        Ok(name_comment_pairs) => {
            let names = name_comment_pairs
                .iter()
                .map(|kv| format!("{}", kv.0))
                .collect::<Vec<String>>();

            let names_validation = validate_secret_names(&names);

            match names_validation {
                Ok(_) => Ok(name_comment_pairs),
                Err(err) => {
                    let mapped_err = map_secret_to_load_set_secrets_error(&err);
                    let error = InputValidationError::LoadEnvironment(mapped_err);
                    Err(error)
                }
            }
        }
        Err(_) => {
            let error = InputValidationError::LoadEnvironment(
                LoadEnvironmentInputValidationError::SetSecretNameValueSeparator,
            );

            Err(error)
        }
    }
}
