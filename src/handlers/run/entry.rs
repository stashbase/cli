use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context};
use log::debug;
use tabled::Tabled;

use crate::{
    api::secrets,
    cmd::secrets::SecretsFileFormat,
    exit::ReportedFailure,
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
    telemetry::event::{classify_output_error, ErrorKind},
    utils::{
        env,
        interaction::{self},
        output::{get_formatted_json_string, ColorizeIfColoredOutput},
        secrets::read_secrets_from_file,
        separator,
        spinner::{Spinner, Streams},
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

/// Ensures the Docker sandbox backend's agent image exists locally,
/// building it (the built-in embedded Dockerfile, or a profile-supplied
/// custom one) on first use. There is no registry to `docker pull` from for
/// either of those, so the only way an installed `stashbase` binary can get
/// them is to build them itself; a plain `sandbox.image` reference, by
/// contrast, needs nothing built here — `docker run` pulls it automatically.
///
/// Returns the image tag `docker run` should use for the agent container.
///
/// In an interactive session, asks before building (an implicit multi-
/// minute `docker build` on first use would otherwise be a surprising side
/// effect of `agent run`). In `--silent` mode there is no one to ask, so
/// this fails closed with instructions rather than silently building or
/// silently running unsandboxed.
fn ensure_docker_sandbox_image_available(
    source: &super::docker_sandbox::AgentImageSource,
    silent: bool,
) -> anyhow::Result<String> {
    let tag = source.image_tag();
    if super::docker_sandbox::sandbox_image_exists(source) {
        return Ok(tag);
    }
    if silent {
        anyhow::bail!(
            "the Docker sandbox image ({tag}) is not built yet; build it once with `docker build -t {tag} <Dockerfile>` or re-run without --silent to be prompted",
        );
    }
    eprintln!();
    let should_build = crate::utils::interaction::confirm_opt(&format!(
        "The Docker sandbox image ({tag}) isn't built yet. Build it now?"
    ))
    .unwrap_or(false);
    // dialoguer can leave the terminal cursor hidden if the prompt is
    // dismissed via Ctrl+C rather than answered normally (a known
    // dialoguer/raw-mode interaction — see the Ctrl+C handler in main.rs
    // for the same workaround applied to other prompts). Restore it
    // unconditionally before deciding what the prompt's outcome was.
    let _ = dialoguer::console::Term::stdout().show_cursor();
    if !should_build {
        anyhow::bail!("Docker sandbox backend selected, but its image was not built");
    }
    eprintln!("Building Docker sandbox image ({tag})...");
    super::docker_sandbox::build_sandbox_image(source)
        .map_err(|error| anyhow::anyhow!("failed to build the Docker sandbox image: {error}"))?;
    eprintln!("Docker sandbox image built.");
    Ok(tag)
}

/// Ensures every image a Docker-backend run will actually need is
/// available, and returns the tag the agent container should use.
///
/// The network-namespace holder (see `start_netns_holder`) always uses the
/// built-in default image, deliberately never a profile's custom
/// `sandbox.image`/`sandbox.dockerfile` — but `ensure_docker_sandbox_image_available`
/// on its own only ensures whichever source the *agent* container resolves
/// to. For a profile using a custom image, that leaves the default image
/// unchecked: if it was never built (a user who only ever runs custom
/// images has no reason to have it), the holder's own `docker run` fails
/// outright since there's no registry to auto-pull it from. This ensures
/// both, skipping the duplicate prompt/build when the agent's own source
/// already *is* the default.
fn ensure_docker_images_available(
    agent_image_source: &super::docker_sandbox::AgentImageSource,
    silent: bool,
) -> anyhow::Result<String> {
    if *agent_image_source != super::docker_sandbox::AgentImageSource::Default {
        ensure_docker_sandbox_image_available(
            &super::docker_sandbox::AgentImageSource::Default,
            silent,
        )?;
    }
    ensure_docker_sandbox_image_available(agent_image_source, silent)
}

fn prepare_run_worktree_in(
    request: Option<&super::worktree::WorktreeRequest>,
    silent: bool,
    cwd: &Path,
    worktrees_root: Option<&Path>,
) -> anyhow::Result<Option<super::worktree::RunWorktree>> {
    let Some(request) = request else {
        return Ok(None);
    };
    let worktree = match request {
        super::worktree::WorktreeRequest::New => {
            super::worktree::create_named_run_worktree(cwd, worktrees_root)
                .map_err(|error| anyhow::anyhow!("failed to create agent worktree: {error}"))?
        }
        super::worktree::WorktreeRequest::Resume(name) => {
            super::worktree::resume_run_worktree(cwd, name, worktrees_root)
                .map_err(|error| anyhow::anyhow!("failed to resume agent worktree: {error}"))?
        }
    };
    if !silent {
        let verb = match request {
            super::worktree::WorktreeRequest::New => "Agent worktree",
            super::worktree::WorktreeRequest::Resume(_) => "Resuming agent worktree",
        };
        eprintln!(
            "{verb}: {} (branch {})",
            worktree.path.display(),
            worktree.branch
        );
        if worktree.source_was_dirty {
            eprintln!(
                "warning: your checkout has uncommitted changes; the agent worktree starts from HEAD without them"
            );
        }
    }
    Ok(Some(worktree))
}

fn prepare_run_worktree(
    request: Option<&super::worktree::WorktreeRequest>,
    silent: bool,
) -> anyhow::Result<Option<super::worktree::RunWorktree>> {
    let cwd = std::env::current_dir()?;
    prepare_run_worktree_in(request, silent, &cwd, None)
}

/// Owns a run's agent worktree, if any, and cleans it up exactly once when
/// dropped — on every exit path, including early `return`s, `?` and
/// panics — so no setup failure can leave a worktree (and its lock)
/// behind. The normal path calls `finish` explicitly to keep the cleanup
/// output where it belongs.
pub(crate) struct RunWorktreeGuard {
    worktree: Option<super::worktree::RunWorktree>,
    silent: bool,
    /// Docker backend: the run's isolated-path volumes are keyed by the
    /// worktree path, so they go away with a removed worktree.
    docker: bool,
}

impl RunWorktreeGuard {
    #[cfg(test)]
    fn none(silent: bool) -> Self {
        Self {
            worktree: None,
            silent,
            docker: false,
        }
    }

    /// What the agent container needs mounted besides the worktree.
    fn git_mounts(&self) -> Option<super::docker_sandbox::GitMounts> {
        self.worktree
            .as_ref()
            .map(super::docker_sandbox::GitMounts::from)
    }

    /// Where the agent should start, when running in a worktree.
    fn workdir(&self) -> Option<&Path> {
        self.worktree
            .as_ref()
            .map(|worktree| worktree.workdir.as_path())
    }

    fn finish(self) {
        drop(self);
    }
}

impl Drop for RunWorktreeGuard {
    fn drop(&mut self) {
        if let Some(worktree) = self.worktree.take() {
            finish_run_worktree(&worktree, self.docker, self.silent);
        }
    }
}

/// `prepare_run_worktree` for the native backend, whose filesystem policy
/// is a deny-list: also adds the paths `RunWorktree::native_protected_paths`
/// names to the run's `deny_write`, so the agent can't touch the user's
/// checkout or the git files host git executes. Fails closed — if the
/// protections can't be computed the run is aborted, and the guard cleans
/// the worktree up as it drops.
fn prepare_native_run_worktree(
    request: Option<&super::worktree::WorktreeRequest>,
    silent: bool,
    denied_write_paths: &mut Vec<String>,
) -> anyhow::Result<RunWorktreeGuard> {
    let guard = RunWorktreeGuard {
        worktree: prepare_run_worktree(request, silent)?,
        silent,
        docker: false,
    };
    if let Some(worktree) = &guard.worktree {
        let paths = worktree.native_protected_paths().map_err(|error| {
            anyhow::anyhow!("failed to protect the checkout for the agent worktree: {error}")
        })?;
        // Generated absolute paths: keep them literal even if the checkout's
        // path happens to contain glob characters.
        denied_write_paths.extend(
            paths
                .iter()
                .map(|path| super::fs_rules::literal_entry(path)),
        );
    }
    Ok(guard)
}

/// `prepare_run_worktree` for the Docker backend. Nothing to add to the
/// policy: the container only ever sees the worktree and the git mounts
/// from `RunWorktreeGuard::git_mounts` (see
/// `docker_sandbox::append_git_mounts`), never the user's checkout.
fn prepare_docker_run_worktree(
    request: Option<&super::worktree::WorktreeRequest>,
    silent: bool,
) -> anyhow::Result<RunWorktreeGuard> {
    Ok(RunWorktreeGuard {
        worktree: prepare_run_worktree(request, silent)?,
        silent,
        docker: true,
    })
}

/// Post-run cleanup. Order matters: pointer files are restored first
/// because every later step runs host git that would otherwise follow a
/// tampered pointer (see `worktree.rs` module docs). Errors are reported,
/// never allowed to mask the run's own exit status.
fn finish_run_worktree(worktree: &super::worktree::RunWorktree, docker: bool, silent: bool) {
    match worktree.restore_pointers() {
        Ok(true) => eprintln!(
            "warning: the agent modified the worktree's git pointer files; they were restored"
        ),
        Ok(false) => {}
        Err(error) => {
            eprintln!(
                "warning: could not verify the agent worktree's git pointer files ({error}); \
                 do not run git in {} until you have checked its .git file",
                worktree.path.display()
            );
            return;
        }
    }
    match worktree.restore_foreign_refs() {
        Ok(changes) => {
            for change in changes {
                eprintln!("warning: {change}");
            }
        }
        Err(error) => eprintln!("warning: failed to check refs after the run: {error}"),
    }
    match worktree.finish() {
        Ok(super::worktree::WorktreeOutcome::Removed) => {
            if docker {
                for error in
                    super::docker_sandbox::remove_isolated_path_volumes_under(&worktree.path)
                {
                    eprintln!("warning: failed to remove isolated path volume {error}");
                }
            }
            if !silent {
                eprintln!("Agent work is on branch {}", worktree.branch);
            }
        }
        Ok(super::worktree::WorktreeOutcome::Kept) => eprintln!(
            "Agent worktree has uncommitted changes and was kept at {} (branch {})",
            worktree.path.display(),
            worktree.branch
        ),
        Err(error) => eprintln!("warning: failed to remove the agent worktree: {error}"),
    }
}

/// Runs an agent through the localhost relay while credentials stay in the
/// control-plane's short-lived remote agent-proxy session.
pub async fn handle_remote_agent_run(
    api_key: String,
    hooks_enabled: bool,
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
    let denied_read_paths = policy.denied_read_paths.clone();
    let mut denied_write_paths = policy.denied_write_paths.clone();
    let allow_network_listeners = policy.allow_network_listeners;
    let backend = policy.backend;
    let agent_image_source = super::docker_sandbox::AgentImageSource::from_profile(
        policy.sandbox_image.as_deref(),
        policy.sandbox_dockerfile.as_deref(),
    );
    let sandbox_memory = policy.sandbox_memory.clone();
    let sandbox_cpus = policy.sandbox_cpus.clone();
    let sandbox_isolated_paths = policy.sandbox_isolated_paths.clone();
    let worktree_request = policy.worktree_request();
    let command_audit_log = audit_log.clone();
    let mut setup_spinner: Option<Spinner> = None;
    let run_worktree;
    let (docker_network, agent_image) = if backend == crate::models::agent::SandboxBackend::Docker {
        // Resolved before the spinner starts: this can print its own
        // interactive "build the image now?" prompt on first use, which
        // must never race a concurrently animating spinner writing to the
        // same stream (see the same reasoning for the proxy-started
        // message below).
        let agent_image = ensure_docker_images_available(&agent_image_source, silent)?;
        run_worktree = prepare_docker_run_worktree(worktree_request.as_ref(), silent)?;
        setup_spinner = (!silent).then(|| {
            crate::utils::spinner::new_spinner("Preparing sandbox network...", Streams::Stderr)
        });
        (
            Some(
                super::docker_sandbox::create_run_network(
                    audit_log.as_ref().map(|log| log.session_id()),
                )
                .map_err(|error| {
                    anyhow::anyhow!("failed to create Docker sandbox network: {error}")
                })?,
            ),
            agent_image,
        )
    } else {
        run_worktree = prepare_native_run_worktree(
            worktree_request.as_ref(),
            silent,
            &mut denied_write_paths,
        )?;
        (None, String::new())
    };
    let proxy_start_result = if let Some(network) = &docker_network {
        super::proxy::Proxy::start_remote_with_hook_and_bind_host(
            remote,
            policy,
            audit_log,
            proxy_port,
            hooks_enabled.then_some(api_key),
            &super::docker_sandbox::proxy_bind_host(network),
        )
        .await
    } else {
        super::proxy::Proxy::start_remote_with_hook(
            remote,
            policy,
            audit_log,
            proxy_port,
            hooks_enabled.then_some(api_key),
        )
        .await
    };
    let proxy = match proxy_start_result {
        Ok(proxy) => proxy,
        Err(error) => {
            if let Some(mut spinner) = setup_spinner.take() {
                spinner.clear();
            }
            if let Some(network) = &docker_network {
                let _ = super::docker_sandbox::remove_run_network(network);
            }
            return Err(error);
        }
    };
    // Same teardown as the other setup failures: `?` here would skip it and
    // leave the proxy, the Docker network and the agent worktree behind.
    let _trusted_ca = match trust_proxy_ca.then(|| proxy.trust_ca()).transpose() {
        Ok(trusted_ca) => trusted_ca,
        Err(error) => {
            if let Some(mut spinner) = setup_spinner.take() {
                spinner.clear();
            }
            proxy.stop().await;
            if let Some(network) = &docker_network {
                let _ = super::docker_sandbox::remove_run_network(network);
            }
            return Err(error);
        }
    };
    // Stop the spinner before any plain `eprintln!` — the spinner redraws its
    // line from a background thread, and interleaving that with ordinary
    // stderr writes garbles both. Re-created below to cover the remaining
    // netns-holder setup phase.
    if let Some(mut spinner) = setup_spinner.take() {
        spinner.clear();
    }
    if !silent {
        let address = proxy.child_env()["HTTP_PROXY"].trim_start_matches("http://");
        eprintln!(
            "Remote agent proxy relay started on localhost:{}",
            address.rsplit(':').next().unwrap_or_default()
        );
        eprintln!("Remote agent proxy session active");
    }
    let mut child_env = if let Some(network) = &docker_network {
        super::docker_sandbox::rewrite_proxy_urls_for_container(
            proxy.child_env(),
            &super::docker_sandbox::proxy_bind_host(network),
            &super::docker_sandbox::proxy_container_host(network),
        )
    } else {
        proxy.child_env().clone()
    };
    let mut setup_spinner = (!silent && docker_network.is_some()).then(|| {
        crate::utils::spinner::new_spinner(
            "Starting network namespace holder and firewall...",
            Streams::Stderr,
        )
    });
    if let Some(network) = &docker_network {
        match super::docker_sandbox::start_netns_holder(network, &child_env) {
            Ok(Some(resolved_proxy_ip)) => {
                // The agent container joins the holder's network namespace
                // via `--network container:<holder>`, which Docker refuses
                // to combine with `--add-host` — so the agent has no way to
                // resolve `host.docker.internal` itself. Point it straight
                // at the already-resolved IP instead, sidestepping the
                // need for any DNS/hosts lookup in the agent container.
                child_env = super::docker_sandbox::rewrite_proxy_urls_for_container(
                    &child_env,
                    &super::docker_sandbox::proxy_container_host(network),
                    &resolved_proxy_ip,
                );
            }
            Ok(None) => {}
            Err(error) => {
                if let Some(mut spinner) = setup_spinner.take() {
                    spinner.clear();
                }
                let _ = super::docker_sandbox::remove_run_network(network);
                proxy.stop().await;
                return Err(anyhow::anyhow!(
                    "failed to start Docker sandbox network namespace holder: {error}"
                ));
            }
        }
    }
    if let Some(mut spinner) = setup_spinner.take() {
        spinner.clear();
    }
    let result = subprocess::run_command_with_filesystem_policy_and_network(
        &cmd,
        args,
        child_env,
        source_env_names,
        sandbox,
        allow_network_listeners,
        true,
        true,
        &denied_read_paths,
        &denied_write_paths,
        command_audit_log,
        backend,
        docker_network.as_ref(),
        &agent_image,
        sandbox_memory.as_deref(),
        sandbox_cpus.as_deref(),
        &sandbox_isolated_paths,
        run_worktree.workdir(),
        run_worktree.git_mounts().as_ref(),
    )
    .await;
    proxy.stop().await;
    if let Some(network) = &docker_network {
        if let Err(error) = super::docker_sandbox::remove_run_network(network) {
            eprintln!(
                "warning: failed to remove Docker sandbox network {}: {error}",
                network.name
            );
        }
    }
    run_worktree.finish();
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
    pub dependency_hooks: bool,
    pub local_session: Option<crate::handlers::agent::sessions::LocalAgentSessionGuard>,
}

/// Errors this prints itself (missing secrets, API errors, a declined prompt)
/// return `ReportedFailure`, so the command exits 1 without printing them twice.
/// The one early `Ok(())` is cancelling the `stashbase.yaml` config selection,
/// which exits 0; for `agent run`, `telemetry::never_launched_error` still
/// catches a run that never launched.
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
        dependency_hooks,
        local_session,
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
            false,
            None,
            local_session,
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

            return Err(ReportedFailure::new(ErrorKind::NotFound));
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

            // The confirmed run below starts its own spinner, so discard this
            // one after clearing the warning line.
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
                return Err(ReportedFailure::new(ErrorKind::NotFound));
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
            dependency_hooks,
            Some(api_key.clone()),
            local_session,
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

    if !needs_remote_fetch(&only, &remote_only, proxy_policy.is_some()) {
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
            dependency_hooks,
            Some(api_key.clone()),
            local_session,
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
        // Matches the root API-key guard: the CLI calls this an authentication error.
        return Err(ReportedFailure::new(ErrorKind::Auth));
    }

    let res = secrets::pull(
        api_key.clone(),
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
        let kind = classify_output_error(&err);
        let formatted_err = err.format_error_output(json_format)?;

        if let Some(mut spinner) = spinner {
            spinner.stop_with_message(&formatted_err);
        } else {
            eprintln!("{}", formatted_err);
        }

        return Err(ReportedFailure::new(kind));
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

                    return Err(ReportedFailure::new(ErrorKind::NotFound));
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
                            dependency_hooks,
                            Some(api_key.clone()),
                            local_session,
                        )
                        .await?;
                    } else {
                        return Err(ReportedFailure::new(ErrorKind::NotFound));
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
                        dependency_hooks,
                        Some(api_key.clone()),
                        local_session,
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
    dependency_hooks: bool,
    hook_api_key: Option<String>,
    local_session: Option<crate::handlers::agent::sessions::LocalAgentSessionGuard>,
) -> anyhow::Result<()> {
    apply_secret_bindings(&mut secrets, secret_bindings);
    let secrets_hash_map = if secret_bindings.is_empty() {
        env::expand_and_inject_env(&mut secrets)
    } else {
        env::expand_env_without_process_override(&mut secrets)
    };

    if !silent {
        let mut success_msg = format!(
            "{} {}",
            "✓".green_if_tty_stderr(),
            loaded_message(secrets.len(), proxy_policy.is_some())
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
    let denied_read_paths = proxy_policy
        .as_ref()
        .map(|policy| policy.denied_read_paths.clone())
        .unwrap_or_default();
    let mut denied_write_paths = proxy_policy
        .as_ref()
        .map(|policy| policy.denied_write_paths.clone())
        .unwrap_or_default();
    let allow_network_listeners = proxy_policy
        .as_ref()
        .is_some_and(|policy| policy.allow_network_listeners);
    let backend = proxy_policy
        .as_ref()
        .map(|policy| policy.backend)
        .unwrap_or_default();
    let agent_image_source = super::docker_sandbox::AgentImageSource::from_profile(
        proxy_policy
            .as_ref()
            .and_then(|policy| policy.sandbox_image.as_deref()),
        proxy_policy
            .as_ref()
            .and_then(|policy| policy.sandbox_dockerfile.as_deref()),
    );
    let sandbox_memory = proxy_policy
        .as_ref()
        .and_then(|policy| policy.sandbox_memory.clone());
    let sandbox_cpus = proxy_policy
        .as_ref()
        .and_then(|policy| policy.sandbox_cpus.clone());
    let worktree_request = proxy_policy
        .as_ref()
        .and_then(super::proxy::ProxyPolicy::worktree_request);
    let sandbox_isolated_paths = proxy_policy
        .as_ref()
        .map(|policy| policy.sandbox_isolated_paths.clone())
        .unwrap_or_default();

    // Proxy mode gives the child placeholders, never the loaded secret values.
    // The temporary proxy owns the placeholder-to-secret mapping until the command exits.
    let command_result = if proxy {
        let command_audit_log = audit_log.clone();
        let mut setup_spinner: Option<Spinner> = None;
        let run_worktree;
        let (docker_network, agent_image) = if backend
            == crate::models::agent::SandboxBackend::Docker
        {
            // Resolved before the spinner starts: this can print its own
            // interactive "build the image now?" prompt on first use, which
            // must never race a concurrently animating spinner writing to
            // the same stream (see the same reasoning for the
            // proxy-started message below).
            let agent_image = ensure_docker_images_available(&agent_image_source, silent)?;
            run_worktree = prepare_docker_run_worktree(worktree_request.as_ref(), silent)?;
            setup_spinner = (!silent).then(|| {
                crate::utils::spinner::new_spinner("Preparing sandbox network...", Streams::Stderr)
            });
            let run_session_id = local_session
                .as_ref()
                .map(|session| session.session_id())
                .or_else(|| audit_log.as_ref().map(|log| log.session_id()));
            (
                Some(
                    super::docker_sandbox::create_run_network(run_session_id).map_err(|error| {
                        anyhow::anyhow!("failed to create Docker sandbox network: {error}")
                    })?,
                ),
                agent_image,
            )
        } else {
            run_worktree = prepare_native_run_worktree(
                worktree_request.as_ref(),
                silent,
                &mut denied_write_paths,
            )?;
            (None, String::new())
        };
        let proxy_start_result = if let Some(network) = &docker_network {
            super::proxy::Proxy::start_with_hook_and_bind_host(
                secrets_hash_map,
                proxy_policy.unwrap_or_else(super::proxy::ProxyPolicy::permissive),
                audit_log,
                proxy_port,
                dependency_hooks.then_some(hook_api_key).flatten(),
                &super::docker_sandbox::proxy_bind_host(network),
            )
            .await
        } else {
            super::proxy::Proxy::start_with_hook(
                secrets_hash_map,
                proxy_policy.unwrap_or_else(super::proxy::ProxyPolicy::permissive),
                audit_log,
                proxy_port,
                dependency_hooks.then_some(hook_api_key).flatten(),
            )
            .await
        };
        let proxy = match proxy_start_result {
            Ok(proxy) => proxy,
            Err(error) => {
                if let Some(mut spinner) = setup_spinner.take() {
                    spinner.clear();
                }
                if let Some(network) = &docker_network {
                    let _ = super::docker_sandbox::remove_run_network(network);
                }
                return Err(error);
            }
        };
        if let Some(session) = &local_session {
            proxy.set_revocation_path(session.path());
        }
        // Same teardown as the other setup failures: `?` here would skip it and
        // leave the proxy, the Docker network and the agent worktree behind.
        let _trusted_ca = match trust_proxy_ca.then(|| proxy.trust_ca()).transpose() {
            Ok(trusted_ca) => trusted_ca,
            Err(error) => {
                if let Some(mut spinner) = setup_spinner.take() {
                    spinner.clear();
                }
                proxy.stop().await;
                if let Some(network) = &docker_network {
                    let _ = super::docker_sandbox::remove_run_network(network);
                }
                return Err(error);
            }
        };
        // Stop the spinner before any plain `eprintln!` — the spinner redraws
        // its line from a background thread, and interleaving that with
        // ordinary stderr writes garbles both. Re-created below to cover
        // the remaining netns-holder setup phase.
        if let Some(mut spinner) = setup_spinner.take() {
            spinner.clear();
        }
        if !silent {
            let address = proxy.child_env()["HTTP_PROXY"].trim_start_matches("http://");
            eprintln!(
                "Agent proxy started on localhost:{}",
                address.rsplit(':').next().unwrap_or_default()
            );
        }
        let mut child_env = if let Some(network) = &docker_network {
            super::docker_sandbox::rewrite_proxy_urls_for_container(
                proxy.child_env(),
                &super::docker_sandbox::proxy_bind_host(network),
                &super::docker_sandbox::proxy_container_host(network),
            )
        } else {
            proxy.child_env().clone()
        };
        let mut setup_spinner = (!silent && docker_network.is_some()).then(|| {
            crate::utils::spinner::new_spinner(
                "Starting network namespace holder and firewall...",
                Streams::Stderr,
            )
        });
        if let Some(network) = &docker_network {
            match super::docker_sandbox::start_netns_holder(network, &child_env) {
                Ok(Some(resolved_proxy_ip)) => {
                    // See the matching comment in handle_remote_agent_run:
                    // the agent container cannot resolve
                    // `host.docker.internal` itself once it joins the
                    // holder's network namespace, so point it at the
                    // already-resolved IP instead.
                    child_env = super::docker_sandbox::rewrite_proxy_urls_for_container(
                        &child_env,
                        &super::docker_sandbox::proxy_container_host(network),
                        &resolved_proxy_ip,
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    if let Some(mut spinner) = setup_spinner.take() {
                        spinner.clear();
                    }
                    let _ = super::docker_sandbox::remove_run_network(network);
                    proxy.stop().await;
                    return Err(anyhow::anyhow!(
                        "failed to start Docker sandbox network namespace holder: {error}"
                    ));
                }
            }
        }
        if let Some(mut spinner) = setup_spinner.take() {
            spinner.clear();
        }
        let git_mounts = run_worktree.git_mounts();
        let command = Box::pin(subprocess::run_command_with_filesystem_policy_and_network(
            &cmd,
            args,
            child_env,
            secret_bindings.keys().cloned().collect(),
            sandbox,
            allow_network_listeners,
            true,
            restrict_stashbase_credentials,
            &denied_read_paths,
            &denied_write_paths,
            command_audit_log,
            backend,
            docker_network.as_ref(),
            &agent_image,
            sandbox_memory.as_deref(),
            sandbox_cpus.as_deref(),
            &sandbox_isolated_paths,
            run_worktree.workdir(),
            git_mounts.as_ref(),
        ));
        let result = command.await;
        proxy.stop().await;
        if let Some(network) = &docker_network {
            if let Err(error) = super::docker_sandbox::remove_run_network(network) {
                eprintln!(
                    "warning: failed to remove Docker sandbox network {}: {error}",
                    network.name
                );
            }
        }
        run_worktree.finish();
        if !silent {
            eprintln!("Agent proxy stopped");
        }
        result
    } else if backend != crate::models::agent::SandboxBackend::Native {
        // The non-proxy path has no proxy/network to attach a Docker
        // sandbox to. Fail closed rather than silently downgrading a
        // profile's requested backend to Native.
        Err(anyhow::anyhow!(
            "the selected sandbox backend requires the agent proxy; re-run with the proxy enabled"
        ))
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

    let _ = dialoguer::console::Term::stdout().show_cursor();
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
/// The line `run` prints once the secrets are loaded. "Egress-only profile"
/// describes an agent profile, so plain `stashbase run` never says it.
fn loaded_message(secret_count: usize, agent_run: bool) -> String {
    let label = if agent_run && secret_count == 0 {
        "Egress-only profile"
    } else {
        "Environment loaded"
    };
    let noun = if secret_count == 1 {
        "secret"
    } else {
        "secrets"
    };
    format!("{label} ({secret_count} {noun})")
}

/// Whether `run` has to fetch secrets from Stashbase. `remote_only` is
/// `only` minus the secrets a local profile file already provides.
///
/// An empty `only` means "every secret" for `stashbase run`, so it always
/// fetches. An agent run (`agent_run`) only ever receives the secrets its
/// profile binds, so an empty `only` there means none.
fn needs_remote_fetch(only: &[String], remote_only: &[String], agent_run: bool) -> bool {
    if only.is_empty() {
        !agent_run
    } else {
        !remote_only.is_empty()
    }
}

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
        apply_secret_bindings, load_run_secrets_from_file, loaded_message,
        merge_remote_and_local_secrets, missing_secret_labels, needs_remote_fetch,
        prepare_local_run_secrets,
    };
    use crate::models::secrets::SecretWithoutComment;
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// `finish_run_worktree` must repair the pointer file before any host
    /// git runs in the worktree, then reset foreign refs, then remove the
    /// (clean) worktree — with the agent's own branch kept.
    #[test]
    fn finish_run_worktree_restores_then_removes() {
        use crate::handlers::run::worktree::{create_run_worktree, test_support};

        let (repo, root) = test_support::fixture();
        let wt = create_run_worktree(&repo, "ags_finish", Some(&root)).unwrap();
        let other_before = test_support::git(&repo, &["rev-parse", "other"]);
        test_support::git(&wt.path, &["commit", "-q", "--allow-empty", "-m", "agent"]);
        let rewritten = test_support::unrelated_commit(&wt.path);
        test_support::git(&wt.path, &["update-ref", "refs/heads/other", &rewritten]);
        std::fs::write(wt.path.join(".git"), "gitdir: /nonexistent\n").unwrap();

        super::finish_run_worktree(&wt, false, true);

        assert!(!wt.path.exists(), "clean worktree removed after repair");
        assert_eq!(
            test_support::git(&repo, &["rev-parse", "other"]),
            other_before
        );
        assert_eq!(
            test_support::git(&repo, &["log", "-1", "--format=%s", "stashbase/ags_finish"]),
            "agent"
        );
    }

    fn guarded_worktree(
        silent: bool,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        super::RunWorktreeGuard,
    ) {
        let (repo, root) = crate::handlers::run::worktree::test_support::fixture();
        let worktree = super::prepare_run_worktree_in(
            Some(&crate::handlers::run::worktree::WorktreeRequest::New),
            true,
            &repo,
            Some(&root),
        )
        .unwrap();
        let path = worktree.as_ref().unwrap().path.clone();
        (
            repo,
            path,
            super::RunWorktreeGuard {
                worktree,
                silent,
                docker: false,
            },
        )
    }

    #[test]
    fn worktree_guard_cleans_up_when_setup_fails_with_an_early_return() {
        let (repo, path, guard) = guarded_worktree(true);
        let setup = move || -> anyhow::Result<()> {
            let _guard = guard;
            anyhow::bail!("proxy failed to start");
        };
        assert!(setup().is_err());
        assert!(!path.exists(), "clean worktree removed on the early return");
        let listing = crate::handlers::run::worktree::test_support::git(
            &repo,
            &["worktree", "list", "--porcelain"],
        );
        assert!(!listing.contains("locked"), "lock released: {listing}");
    }

    #[test]
    fn worktree_guard_cleans_up_on_panic() {
        let (_, path, guard) = guarded_worktree(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = guard;
            panic!("unexpected failure mid-run");
        }));
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn worktree_guard_finishes_once() {
        let (_, path, guard) = guarded_worktree(true);
        assert_eq!(guard.workdir(), Some(path.as_path()));
        guard.finish();
        assert!(!path.exists());
        // Dropping an empty guard is a no-op.
        super::RunWorktreeGuard::none(true).finish();
    }

    #[test]
    fn prepare_run_worktree_resumes_a_kept_worktree() {
        use crate::handlers::run::worktree::{test_support, WorktreeOutcome, WorktreeRequest};
        let (repo, root) = test_support::fixture();
        let first =
            super::prepare_run_worktree_in(Some(&WorktreeRequest::New), true, &repo, Some(&root))
                .unwrap()
                .unwrap();
        std::fs::write(first.path.join("wip.txt"), "x").unwrap();
        first.restore_pointers().unwrap();
        assert_eq!(first.finish().unwrap(), WorktreeOutcome::Kept);
        let name = first
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let resumed = super::prepare_run_worktree_in(
            Some(&WorktreeRequest::Resume(name)),
            true,
            &repo,
            Some(&root),
        )
        .unwrap()
        .unwrap();
        assert_eq!(resumed.path, first.path);
        assert!(resumed.path.join("wip.txt").exists());
    }

    #[test]
    fn prepare_run_worktree_is_a_no_op_when_disabled() {
        assert!(
            super::prepare_run_worktree_in(None, true, std::path::Path::new("/"), None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prepare_run_worktree_creates_a_named_worktree_when_enabled() {
        let (repo, root) = crate::handlers::run::worktree::test_support::fixture();
        let worktree = super::prepare_run_worktree_in(
            Some(&crate::handlers::run::worktree::WorktreeRequest::New),
            true,
            &repo,
            Some(&root),
        )
        .unwrap()
        .expect("worktree created");
        assert!(worktree
            .path
            .starts_with(crate::handlers::run::worktree::canonicalize(&root).unwrap()));
        assert!(worktree.branch.starts_with("stashbase/"));
    }

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
    fn only_an_agent_run_without_secrets_is_called_egress_only() {
        assert_eq!(loaded_message(0, true), "Egress-only profile (0 secrets)");
        assert_eq!(loaded_message(1, true), "Environment loaded (1 secret)");
        assert_eq!(loaded_message(0, false), "Environment loaded (0 secrets)");
        assert_eq!(loaded_message(10, false), "Environment loaded (10 secrets)");
    }

    #[test]
    fn run_without_only_fetches_every_secret() {
        assert!(needs_remote_fetch(&[], &[], false));
    }

    #[test]
    fn an_agent_run_never_fetches_secrets_it_does_not_bind() {
        assert!(!needs_remote_fetch(&[], &[], true));
    }

    #[test]
    fn requested_secrets_are_fetched_unless_a_local_file_provides_them_all() {
        let only = ["GITHUB_TOKEN".to_owned()];
        for agent_run in [false, true] {
            assert!(needs_remote_fetch(&only, &only, agent_run));
            assert!(!needs_remote_fetch(&only, &[], agent_run));
        }
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
