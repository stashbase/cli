use std::collections::HashSet;

use anyhow::Result;

use crate::cmd::agent::{
    AgentDockerBuildCommand, AgentDockerCleanupCommand, AgentDockerStatusCommand,
};
use crate::handlers::run::docker_sandbox::ExistingRunNetwork;

/// Whether `network` should be skipped as a cleanup candidate because it's
/// tied to a session this machine still has a live local record for. Pure
/// and Docker-free so it's directly unit-testable; the Docker-backed
/// listing and the local-session lookup both happen in the caller.
fn belongs_to_a_live_local_session(
    network: &ExistingRunNetwork,
    local_sessions: &HashSet<String>,
) -> bool {
    network
        .session_id
        .as_deref()
        .is_some_and(|id| local_sessions.contains(id))
}

/// Every per-run Docker network still present on this machine, paired with
/// whether it's tied to a session this machine still has a live local
/// record for. Shared by `status` (which reports both) and `cleanup`
/// (which only ever acts on the ones that aren't).
fn list_networks_with_liveness() -> Result<Vec<(ExistingRunNetwork, bool)>> {
    let networks = crate::handlers::run::docker_sandbox::list_run_networks()
        .map_err(|error| anyhow::anyhow!("failed to list Docker sandbox networks: {error}"))?;
    let local_sessions = crate::handlers::agent_sessions::list_local_sessions()
        .unwrap_or_default()
        .into_iter()
        .map(|session| session.session_id)
        .collect::<HashSet<_>>();
    Ok(networks
        .into_iter()
        .map(|network| {
            let live = belongs_to_a_live_local_session(&network, &local_sessions);
            (network, live)
        })
        .collect())
}

/// Lists every Docker sandbox network currently present on this machine,
/// regardless of whether it belongs to a live session — a read-only view of
/// what `cleanup` would consider, without removing anything.
pub async fn handle_docker_status_command(
    _command: AgentDockerStatusCommand,
    raw_output: bool,
) -> Result<()> {
    let entries = list_networks_with_liveness()?;
    if raw_output {
        let json = entries
            .iter()
            .map(|(network, live)| {
                serde_json::json!({
                    "name": network.name,
                    "session_id": network.session_id,
                    "created_at": network.created_at,
                    "live_local_session": live,
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }
    if entries.is_empty() {
        println!("No Docker sandbox networks currently present.");
        return Ok(());
    }
    for (network, live) in &entries {
        println!(
            "{}  created {}{}",
            network.name,
            if network.created_at.is_empty() {
                "unknown"
            } else {
                network.created_at.as_str()
            },
            if *live { "  (live local session)" } else { "" }
        );
    }
    Ok(())
}

/// Builds (or rebuilds, with `--force`) either the default Docker sandbox
/// image or, with `--profile`, that profile's own `sandbox.image` /
/// `sandbox.dockerfile` — mutually exclusive targets, never both. A
/// profile's custom image is otherwise built automatically the first time
/// that profile actually runs (see `ensure_docker_sandbox_image_available`);
/// this command exists to do that ahead of time, or to force a refresh
/// (new apt packages, a security patch) without having to `docker rmi` it
/// by hand first.
pub async fn handle_docker_build_command(
    command: AgentDockerBuildCommand,
    global_config: &crate::models::config::Config,
    silent: bool,
) -> Result<()> {
    if let Some(error) = crate::handlers::run::docker_sandbox::docker_enforcement_error() {
        anyhow::bail!("Docker sandbox backend unavailable: {error}");
    }

    let source = match &command.profile {
        None => crate::handlers::run::docker_sandbox::AgentImageSource::Default,
        Some(profile_name) => {
            let global_profile = || {
                global_config
                    .agent_profiles
                    .as_ref()
                    .and_then(|profiles| profiles.get(profile_name))
                    .cloned()
            };
            let profile = match command.profile_source {
                crate::cmd::agent::AgentProfileSource::Global => global_profile(),
                crate::cmd::agent::AgentProfileSource::Directory => {
                    crate::config::config::get_directory_agent_profile(profile_name)?
                        .map(|loaded| loaded.profile)
                }
                crate::cmd::agent::AgentProfileSource::Auto => {
                    crate::config::config::get_directory_agent_profile(profile_name)?
                        .map(|loaded| loaded.profile)
                        .or_else(global_profile)
                }
            };
            let Some(profile) = profile else {
                anyhow::bail!("Agent profile '{profile_name}' was not found.");
            };
            crate::handlers::run::docker_sandbox::AgentImageSource::from_profile(
                profile.sandbox.image.as_deref(),
                profile.sandbox.dockerfile.as_deref(),
            )
        }
    };

    if matches!(
        source,
        crate::handlers::run::docker_sandbox::AgentImageSource::Image(_)
    ) {
        if !silent {
            println!(
                "Profile '{}' uses a pre-built image reference ({}); nothing to build — `docker run` pulls it automatically if it isn't already present locally.",
                command.profile.as_deref().unwrap_or_default(),
                source.image_tag()
            );
        }
        return Ok(());
    }

    if !command.force && crate::handlers::run::docker_sandbox::sandbox_image_exists(&source) {
        if !silent {
            println!(
                "Docker sandbox image ({}) already exists. Use --force to rebuild it.",
                source.image_tag()
            );
        }
        return Ok(());
    }
    if !silent {
        println!("Building Docker sandbox image ({})...", source.image_tag());
    }
    crate::handlers::run::docker_sandbox::build_sandbox_image(&source)
        .map_err(|error| anyhow::anyhow!("failed to build the Docker sandbox image: {error}"))?;
    if !silent {
        println!("Docker sandbox image built.");
    }
    Ok(())
}

/// Finds Docker sandbox networks (and their paired containers) left behind
/// by a run that didn't tear down cleanly, and removes them after
/// confirmation.
///
/// A network still existing when nothing is actively using it is always a
/// leftover from a crash or `SIGKILL` — every normal exit path, including
/// Ctrl+C, tears its network down as part of `agent run` itself (see
/// `remove_run_network`). What this command *cannot* know on its own is
/// whether a network belongs to a run that's still genuinely in progress —
/// a local run in progress is cross-checked against this machine's tracked
/// sessions and skipped automatically, but a remote run has no local
/// artifact to check against at all. For that reason this command lists
/// what it found (including how long ago each network was created) and
/// asks for confirmation rather than deleting automatically — the person
/// running it is expected to recognize whether they have a run legitimately
/// in progress right now.
pub async fn handle_docker_cleanup_command(
    command: AgentDockerCleanupCommand,
    silent: bool,
) -> Result<()> {
    if let Some(error) = crate::handlers::run::docker_sandbox::docker_enforcement_error() {
        anyhow::bail!("Docker sandbox backend unavailable: {error}");
    }

    let candidates: Vec<_> = list_networks_with_liveness()?
        .into_iter()
        .filter(|(_, live)| !live)
        .map(|(network, _)| network)
        .collect();

    if candidates.is_empty() {
        if !silent {
            println!("No leftover Docker sandbox resources found.");
        }
        return Ok(());
    }

    if !silent {
        println!(
            "Found {} leftover Docker sandbox network(s) not tied to a live local session:\n",
            candidates.len()
        );
        for network in &candidates {
            println!(
                "  {} (created {})",
                network.name,
                if network.created_at.is_empty() {
                    "unknown"
                } else {
                    network.created_at.as_str()
                }
            );
        }
        println!(
            "\nA remote run has no local record to check against, so a network here could \
             still belong to a remote session genuinely in progress right now — remove only \
             what you recognize as no longer running."
        );
    }

    let should_remove = if command.yes {
        true
    } else if silent {
        anyhow::bail!(
            "{} leftover Docker sandbox network(s) found; re-run with --yes to remove them, or without --silent to be prompted",
            candidates.len()
        );
    } else {
        let confirmed = crate::utils::interaction::confirm_opt(&format!(
            "Remove all {} network(s) listed above?",
            candidates.len()
        ))
        .unwrap_or(false);
        let _ = dialoguer::console::Term::stdout().show_cursor();
        confirmed
    };

    if !should_remove {
        if !silent {
            println!("Nothing removed.");
        }
        return Ok(());
    }

    let mut failures = Vec::new();
    for network in &candidates {
        let run_network = crate::handlers::run::docker_sandbox::DockerRunNetwork {
            name: network.name.clone(),
            gateway_ip: String::new(),
        };
        match crate::handlers::run::docker_sandbox::remove_run_network(&run_network) {
            Ok(()) => {
                if !silent {
                    println!("Removed {}", network.name);
                }
            }
            // Benign race: something else (a concurrently finishing
            // legitimate run, or a second `cleanup` invocation) already
            // removed it between our listing and this removal attempt —
            // the end state we wanted is already true.
            Err(error) if error.contains("not found") => {
                if !silent {
                    println!("Already removed: {}", network.name);
                }
            }
            Err(error) => failures.push(format!("{}: {error}", network.name)),
        }
    }

    if !failures.is_empty() {
        anyhow::bail!(
            "failed to remove {} network(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(session_id: Option<&str>) -> ExistingRunNetwork {
        ExistingRunNetwork {
            name: format!(
                "stashbase-agent-run-{}",
                session_id.unwrap_or("00000000-0000-0000-0000-000000000000")
            ),
            session_id: session_id.map(str::to_owned),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn skips_a_network_whose_session_is_still_tracked_locally() {
        let local_sessions = HashSet::from(["ags_live".to_owned()]);
        assert!(belongs_to_a_live_local_session(
            &network(Some("ags_live")),
            &local_sessions
        ));
    }

    #[test]
    fn does_not_skip_a_network_with_no_matching_local_session() {
        let local_sessions = HashSet::from(["ags_live".to_owned()]);
        assert!(!belongs_to_a_live_local_session(
            &network(Some("ags_dead")),
            &local_sessions
        ));
    }

    #[test]
    fn does_not_skip_a_network_with_no_session_id_at_all() {
        // The pre-session-naming fallback (a bare UUID) never matches a
        // local session id, so it's always treated as a candidate.
        let local_sessions = HashSet::from(["ags_live".to_owned()]);
        assert!(!belongs_to_a_live_local_session(
            &network(None),
            &local_sessions
        ));
    }

    #[test]
    fn does_not_skip_anything_when_there_are_no_local_sessions_at_all() {
        let local_sessions = HashSet::new();
        assert!(!belongs_to_a_live_local_session(
            &network(Some("ags_anything")),
            &local_sessions
        ));
    }
}
