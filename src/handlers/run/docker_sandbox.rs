use std::path::PathBuf;

pub(crate) fn docker_binary_available() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("docker").is_file()))
}

/// Checks whether the Docker daemon is reachable, returning its reported
/// server version on success (used by `agent docker doctor` to show what
/// version is actually running, not just that a check passed).
pub(crate) fn docker_daemon_version() -> Result<String, String> {
    let output = std::process::Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .map_err(|error| format!("failed to run `docker info`: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Checks whether the Docker sandbox backend can run here. Returns `None`
/// when Docker is installed and the daemon is reachable, `Some(message)`
/// otherwise. Callers must fail the run closed on `Some` — never fall back
/// to an unsandboxed execution path.
pub(crate) fn docker_enforcement_error() -> Option<String> {
    if !docker_binary_available() {
        return Some(
            "the Docker sandbox backend requires the `docker` CLI to be installed and on PATH"
                .to_owned(),
        );
    }
    match docker_daemon_version() {
        Ok(_) => None,
        Err(detail) => Some(format!(
            "the Docker sandbox backend requires a reachable Docker daemon: {detail}"
        )),
    }
}

/// Named after the run's own session id (the same `ags_...` id shown in
/// "Agent session"/"Audit session" and used for the audit log filename) so
/// a stray container or network left behind after a crash can be traced
/// back to the session that created it, instead of an unrelated random
/// UUID. Falls back to a fresh UUID only when no session id is available
/// (e.g. audit logging disabled and no local session guard, which
/// shouldn't happen in practice for either the local or remote run paths).
fn generate_run_network_name(session_id: Option<&str>) -> String {
    let suffix = session_id
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    format!("stashbase-agent-run-{suffix}")
}

#[derive(Debug, Clone)]
pub(crate) struct DockerRunNetwork {
    pub name: String,
    pub gateway_ip: String,
}

fn extract_gateway_from_inspect(json: &str) -> Result<String, String> {
    let parsed: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("invalid `docker network inspect` output: {error}"))?;
    parsed
        .get(0)
        .and_then(|network| network.get("IPAM"))
        .and_then(|ipam| ipam.get("Config"))
        .and_then(|config| config.get(0))
        .and_then(|entry| entry.get("Gateway"))
        .and_then(|gateway| gateway.as_str())
        .map(|gateway| gateway.to_owned())
        .ok_or_else(|| "`docker network inspect` did not report a gateway address".to_owned())
}

/// Creates a fresh, isolated Docker bridge network for one `agent run`
/// invocation. The credential proxy binds to the returned gateway address
/// so only the container attached to this network can reach it.
///
/// On native Linux this network is created `--internal`: Docker drops the
/// outbound NAT/masquerade rule that would otherwise let the container
/// reach the wider internet or LAN directly, while containers can still
/// reach the network's own gateway address (a directly-attached bridge
/// interface, not a routed hop) — so the proxy stays reachable. A plain
/// `--driver bridge` network without this flag gives the container full
/// internet/LAN egress no different from running on the host's own
/// network, which would defeat the point of a Docker-specific backend.
///
/// `--internal` is Linux-only here because it also blocks the
/// `host.docker.internal` route Docker Desktop (macOS/Windows) uses to let
/// a container reach the host proxy at all (see `proxy_bind_host`) — on
/// Desktop this backend currently cannot offer kernel-enforced network
/// containment beyond what `HTTPS_PROXY`/`HTTP_PROXY` convention already
/// gives the native backend, and `docs/agent-profiles.md` says so.
fn network_create_args(name: &str) -> Vec<String> {
    let mut args = vec![
        "network".to_owned(),
        "create".to_owned(),
        "--driver".to_owned(),
        "bridge".to_owned(),
    ];
    if cfg!(target_os = "linux") {
        args.push("--internal".to_owned());
    }
    args.push(name.to_owned());
    args
}

pub(crate) fn create_run_network(session_id: Option<&str>) -> Result<DockerRunNetwork, String> {
    let name = generate_run_network_name(session_id);
    let create = std::process::Command::new("docker")
        .args(network_create_args(&name))
        .output()
        .map_err(|error| format!("failed to run `docker network create`: {error}"))?;
    if !create.status.success() {
        return Err(String::from_utf8_lossy(&create.stderr).trim().to_owned());
    }
    let inspect = std::process::Command::new("docker")
        .args(["network", "inspect", &name])
        .output()
        .map_err(|error| format!("failed to run `docker network inspect`: {error}"))?;
    if !inspect.status.success() {
        let _ = remove_run_network(&DockerRunNetwork {
            name: name.clone(),
            gateway_ip: String::new(),
        });
        return Err(String::from_utf8_lossy(&inspect.stderr).trim().to_owned());
    }
    let gateway_ip = extract_gateway_from_inspect(&String::from_utf8_lossy(&inspect.stdout))
        .inspect_err(|_| {
            let _ = remove_run_network(&DockerRunNetwork {
                name: name.clone(),
                gateway_ip: String::new(),
            });
        })?;
    Ok(DockerRunNetwork { name, gateway_ip })
}

/// Name of the short-lived helper container that holds the network
/// namespace the agent container joins (see `start_netns_holder`).
/// Deterministic from the network name so no extra state needs to be
/// threaded through the run — every caller that needs it (starting it,
/// joining it, tearing it down) can derive it the same way.
fn netns_holder_name(network: &DockerRunNetwork) -> String {
    format!("{}-netns-holder", network.name)
}

fn parse_proxy_host_port(
    env_vars: &std::collections::HashMap<String, String>,
) -> Option<(String, String)> {
    let proxy_url = env_vars
        .get("HTTPS_PROXY")
        .or_else(|| env_vars.get("HTTP_PROXY"))?;
    let hostport = proxy_url.split("://").nth(1)?;
    let host = hostport.split(&[':', '/'][..]).next()?;
    let port = hostport.split(':').nth(1)?.split('/').next()?;
    Some((host.to_owned(), port.to_owned()))
}

/// Starts a short-lived helper container attached to `network` that holds
/// `CAP_NET_ADMIN` just long enough to install one `iptables` rule
/// (default-DROP outbound, exceptions only for loopback and the
/// credential proxy's specific address/port), then blocks forever holding
/// the network namespace open. The actual agent container later joins this
/// exact namespace via `--network container:<holder>` (see
/// `docker_run_command`) with zero added capabilities of its own — network
/// namespace rules are shared by anything attached to that namespace, but
/// the *capability* to modify them is not, so the agent process can use
/// the firewall but never touch it.
///
/// This exists because `setpriv`/capability-bounding-set tricks to strip
/// `NET_ADMIN` from the agent container itself after setup turned out not
/// to work on Docker Desktop: granting the container `CAP_SETPCAP` (needed
/// to modify its own bounding set at all) is silently zeroed out there —
/// confirmed directly, not assumed. Splitting privileged setup into a
/// separate container sidesteps that limitation entirely: the agent
/// container never needs `CAP_SETPCAP`, `CAP_NET_ADMIN`, or root, on any
/// platform.
///
/// Returns the proxy's resolved IP address when a proxy was configured, so
/// the caller can point the agent container directly at that IP instead of
/// a hostname — the agent joins this namespace via
/// `--network container:<holder>`, which is incompatible with `--add-host`
/// (Docker rejects the combination outright), so the agent container has
/// no way to resolve `host.docker.internal` itself. Using the
/// already-resolved IP sidesteps needing DNS/hosts resolution in the agent
/// container at all.
pub(crate) fn start_netns_holder(
    network: &DockerRunNetwork,
    proxy_env_vars: &std::collections::HashMap<String, String>,
) -> Result<Option<String>, String> {
    let holder_name = netns_holder_name(network);
    let start = std::process::Command::new("docker")
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            &holder_name,
            "--network",
            &network.name,
            "--add-host",
            "host.docker.internal:host-gateway",
            // Docker's embedded DNS resolver (127.0.0.11) forwards
            // unresolved lookups to a real upstream server on the host
            // side — outside this container's own network namespace
            // entirely, so no iptables OUTPUT rule inside the container can
            // ever see or block that traffic (confirmed directly: even
            // with a default-DROP policy and no ACCEPT rule for port 53 at
            // all, a lookup for an arbitrary external hostname still
            // succeeded). That's a live data-exfiltration channel — an
            // agent can encode secrets in a query name to a
            // domain it controls and have Docker itself relay it out.
            // Pointing the resolver's upstream at a blackhole address
            // closes it: local lookups (`host.docker.internal` via the
            // `--add-host` above) still work since those resolve from
            // `/etc/hosts`, never touching the upstream forwarder at all.
            // The agent joins this container's network namespace and
            // inherits this same DNS config — it doesn't need real DNS
            // either, since its proxy address is already a resolved raw IP
            // (see `rewrite_proxy_urls_for_container`).
            "--dns",
            "0.0.0.0",
            "--cap-drop",
            "ALL",
            "--cap-add",
            "NET_ADMIN",
            "--security-opt",
            "no-new-privileges",
            DEFAULT_SANDBOX_IMAGE,
            "sleep",
            "infinity",
        ])
        .output()
        .map_err(|error| {
            format!("failed to run `docker run` for the network namespace holder: {error}")
        })?;
    if !start.status.success() {
        return Err(String::from_utf8_lossy(&start.stderr).trim().to_owned());
    }

    let Some((proxy_host, proxy_port)) = parse_proxy_host_port(proxy_env_vars) else {
        // No proxy configured for this run — leave the holder's network
        // namespace at Docker's default (unrestricted) rather than
        // guessing at a policy. The Docker backend always runs with the
        // proxy in practice; this is a defensive fallback, not the normal
        // path.
        return Ok(None);
    };
    let setup_script = format!(
        "set -e\n\
         proxy_ip=$(getent hosts '{proxy_host}' 2>/dev/null | awk '{{print $1}}' | head -1)\n\
         if [ -z \"$proxy_ip\" ]; then proxy_ip='{proxy_host}'; fi\n\
         iptables -P OUTPUT DROP\n\
         iptables -A OUTPUT -o lo -j ACCEPT\n\
         iptables -A OUTPUT -d \"$proxy_ip\" -p tcp --dport '{proxy_port}' -j ACCEPT\n\
         \n\
         # Verify the rule actually took effect before trusting it, rather\n\
         # than assuming `iptables` exiting 0 means the running kernel\n\
         # honored it (inspired by Anthropic's own Claude Code devcontainer\n\
         # firewall script, which does the same kind of self-check). A\n\
         # known-arbitrary host must be unreachable, and the proxy itself\n\
         # must still be reachable — either failing means this run is not\n\
         # actually contained and must not proceed.\n\
         set +e\n\
         # 192.0.2.1 is TEST-NET-1 (RFC 5737) — reserved for documentation\n\
         # and testing, never routed on the real internet. Used here purely\n\
         # as an arbitrary destination the firewall must never let through,\n\
         # deliberately not a live third party's real IP (e.g. a public DNS\n\
         # resolver), so this check's correctness never depends on what\n\
         # that company's infrastructure happens to be doing right now.\n\
         #\n\
         # DROP (vs. REJECT) means a blocked connection gets no response at\n\
         # all, so this waits out its own timeout on the success path (the\n\
         # firewall is working) — that wait is pure per-run startup latency,\n\
         # so keep it as short as still reliably catches a real leak. A SYN\n\
         # to a genuinely reachable host resolves in low tens of ms even over\n\
         # the public internet, let alone from a container's own network\n\
         # stack, so 300ms leaves ample margin above any real response time\n\
         # while capping the wasted wait on the (expected) blocked outcome.\n\
         curl -s -m 0.3 -o /dev/null 'http://192.0.2.1/'\n\
         arbitrary_reachable=$?\n\
         curl -s -m 3 -o /dev/null \"http://$proxy_ip:{proxy_port}/\"\n\
         proxy_reachable=$?\n\
         set -e\n\
         \n\
         if [ \"$arbitrary_reachable\" -eq 0 ]; then\n\
             echo 'firewall verification failed: an arbitrary external host was reachable' >&2\n\
             exit 1\n\
         fi\n\
         # curl exit 7 = couldn't connect, 28 = timeout — both mean the\n\
         # proxy's own ACCEPT rule didn't take effect. Any other non-zero\n\
         # exit (e.g. a protocol complaint about a non-HTTP response from\n\
         # the forward-proxy port) still proves the TCP connection itself\n\
         # succeeded, which is all this check needs.\n\
         if [ \"$proxy_reachable\" -eq 7 ] || [ \"$proxy_reachable\" -eq 28 ]; then\n\
             echo \"firewall verification failed: proxy unreachable (curl exit $proxy_reachable)\" >&2\n\
             exit 1\n\
         fi\n\
         \n\
         echo \"$proxy_ip\"\n"
    );
    // `docker exec` immediately after `docker run -d` can race the
    // container's own network setup (DNS in particular isn't always ready
    // the instant the process starts) — retry briefly rather than treat a
    // transient race as a hard failure.
    let mut last_error = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let setup = std::process::Command::new("docker")
            .args(["exec", &holder_name, "sh", "-c", &setup_script])
            .output()
            .map_err(|error| {
                format!("failed to run the network namespace holder's firewall setup: {error}")
            })?;
        if setup.status.success() {
            let resolved_ip = String::from_utf8_lossy(&setup.stdout).trim().to_owned();
            return Ok(Some(resolved_ip));
        }
        last_error = String::from_utf8_lossy(&setup.stderr).trim().to_owned();
    }
    stop_container_if_running(&holder_name);
    Err(last_error)
}

/// Stops and removes the container for this run, if it's still around. The
/// agent container shares its name with the network (`docker_run_command`
/// passes `--name network.name`), so no separate identifier needs to be
/// tracked; the holder's name is derived the same deterministic way (see
/// `netns_holder_name`). Best-effort: a container that already exited and
/// self-removed (the common case — both containers run with `--rm`) has
/// nothing to stop, which is not an error.
///
/// Uses `docker rm -f` rather than `docker stop`: both containers run with
/// `--rm`, so `stop` alone only *starts* the daemon's asynchronous
/// self-removal — it does not wait for the container to actually be gone,
/// which left a real race where `docker network rm` (called right after)
/// could still see the container's endpoint as attached and fail with
/// "has active endpoints". `rm -f` stops and removes synchronously in one
/// call, so by the time this returns the container and its network
/// attachment are actually gone.
fn stop_container_if_running(name: &str) {
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", name])
        .output();
}

/// Force-disconnects a container from a network, ignoring errors (the
/// common case is the container is already gone, in which case there's
/// nothing to disconnect).
fn force_disconnect(network_name: &str, container_name: &str) {
    let _ = std::process::Command::new("docker")
        .args(["network", "disconnect", "-f", network_name, container_name])
        .output();
}

/// Removes a per-run network created by `create_run_network`. Best-effort
/// overall: failure here should not mask the underlying run's exit status
/// — callers should log and continue.
///
/// Both the agent and holder containers are stopped/removed first (see
/// `stop_container_if_running`), and the disconnect+remove sequence is
/// retried a few times with a short delay — confirmed necessary, not just
/// defensive: this Docker setup was observed leaving a network's own
/// bookkeeping pointing at a holder container's endpoint as still "active"
/// immediately after that container was already fully removed (`docker
/// inspect` on it returned "no such object"), causing both a bare `docker
/// network rm` *and* an immediate `disconnect` + `rm` attempt right after
/// removal to fail with "has active endpoints" — the same disconnect+rm
/// sequence reliably succeeds once retried a moment later, once the
/// daemon's own bookkeeping has caught up with the removal it already
/// performed.
pub(crate) fn remove_run_network(network: &DockerRunNetwork) -> Result<(), String> {
    stop_container_if_running(&network.name);
    stop_container_if_running(&netns_holder_name(network));

    let mut last_error = String::new();
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        force_disconnect(&network.name, &network.name);
        force_disconnect(&network.name, &netns_holder_name(network));
        let output = std::process::Command::new("docker")
            .args(["network", "rm", &network.name])
            .output()
            .map_err(|error| format!("failed to run `docker network rm`: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        last_error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    }
    Err(last_error)
}

/// Prefix shared by every per-run Docker network this backend creates —
/// used both to name new networks and to find existing ones left behind by
/// a run that didn't tear down cleanly (a crash or `SIGKILL` of the
/// `stashbase` process itself, which no normal exit path — including
/// Ctrl+C — leaves behind).
const RUN_NETWORK_NAME_PREFIX: &str = "stashbase-agent-run-";

/// A per-run Docker network still present on this machine, found by name
/// rather than tracked in memory (this process may not be the one that
/// created it) — see `list_run_networks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExistingRunNetwork {
    pub name: String,
    /// The session id embedded in the network's name (`ags_...`), or
    /// `None` for the pre-session-naming fallback (a bare UUID) — see
    /// `generate_run_network_name`.
    pub session_id: Option<String>,
    /// RFC 3339 creation timestamp, straight from `docker network inspect`.
    pub created_at: String,
}

/// Lists every Docker network this backend has ever created that still
/// exists, regardless of which process (or machine session) created it.
/// Used by `agent docker cleanup` to find networks a crashed run left
/// behind; deliberately does not attempt to guess which ones are still
/// legitimately in use by a live run elsewhere — that judgment is left to
/// the caller (comparing against locally tracked sessions, prompting the
/// user, etc.), since this function has no way to know about a live
/// *remote* run's session at all.
pub(crate) fn list_run_networks() -> Result<Vec<ExistingRunNetwork>, String> {
    let list = std::process::Command::new("docker")
        .args([
            "network",
            "ls",
            "--filter",
            &format!("name={RUN_NETWORK_NAME_PREFIX}"),
            "--format",
            "{{.Name}}",
        ])
        .output()
        .map_err(|error| format!("failed to run `docker network ls`: {error}"))?;
    if !list.status.success() {
        return Err(String::from_utf8_lossy(&list.stderr).trim().to_owned());
    }
    String::from_utf8_lossy(&list.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        // `docker network ls --filter name=X` matches X anywhere in the
        // name, not just as a prefix — filter again to be exact.
        .filter(|name| name.starts_with(RUN_NETWORK_NAME_PREFIX))
        .map(|name| {
            let inspect = std::process::Command::new("docker")
                .args(["network", "inspect", name, "--format", "{{.Created}}"])
                .output()
                .map_err(|error| format!("failed to run `docker network inspect`: {error}"))?;
            let created_at = if inspect.status.success() {
                String::from_utf8_lossy(&inspect.stdout).trim().to_owned()
            } else {
                // The network could have been removed between the `ls` and
                // this `inspect` (e.g. a concurrent run finishing normally)
                // — report it as unknown rather than failing the whole
                // listing over a race that isn't this caller's problem.
                String::new()
            };
            let session_id = name
                .strip_prefix(RUN_NETWORK_NAME_PREFIX)
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            Ok(ExistingRunNetwork {
                name: name.to_owned(),
                session_id,
                created_at,
            })
        })
        .collect()
}

/// The host address the credential proxy should bind to for this Docker
/// run. On Docker Desktop (macOS/Windows), containers run inside a VM and
/// cannot reach a per-run bridge network's gateway address from the host
/// side — the host process cannot even bind to it (`docker network
/// inspect`'s gateway is only routable inside the Desktop VM). Loopback
/// plus the `host.docker.internal` hostname (which Docker Desktop resolves
/// back to the host) is the supported bridge there. On native Linux
/// Docker, the bridge network's gateway is a real host interface, so
/// binding to it directly keeps the proxy reachable only from this run's
/// isolated network rather than every interface on the host.
pub(crate) fn proxy_bind_host(network: &DockerRunNetwork) -> String {
    if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        "127.0.0.1".to_owned()
    } else {
        network.gateway_ip.clone()
    }
}

/// The host the *container* should use to reach the proxy bound via
/// `proxy_bind_host`. See that function's doc comment for why this differs
/// by platform.
pub(crate) fn proxy_container_host(network: &DockerRunNetwork) -> String {
    if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        "host.docker.internal".to_owned()
    } else {
        network.gateway_ip.clone()
    }
}

/// Env vars whose value is a `http://<bind-host>:<port>[/path]` proxy URL
/// that the container needs to reach at a different host than the proxy
/// actually bound to (see `proxy_bind_host`'s doc comment).
const PROXY_URL_ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "http_proxy",
    "https_proxy",
    crate::api::dependencies::HOOK_BROKER_URL_ENV,
];

/// Rewrites the proxy's child-process env vars so the container reaches the
/// proxy at `container_host` instead of whatever host the proxy actually
/// bound to (`bind_host`) — the two differ on Docker Desktop. Only known
/// proxy-URL keys are rewritten; opaque secret placeholders are left
/// untouched even if they happen to contain the bind host as a substring.
pub(crate) fn rewrite_proxy_urls_for_container(
    env_vars: &std::collections::HashMap<String, String>,
    bind_host: &str,
    container_host: &str,
) -> std::collections::HashMap<String, String> {
    if bind_host == container_host {
        return env_vars.clone();
    }
    env_vars
        .iter()
        .map(|(key, value)| {
            if PROXY_URL_ENV_KEYS.contains(&key.as_str()) {
                (key.clone(), value.replacen(bind_host, container_host, 1))
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

pub(crate) const DEFAULT_SANDBOX_IMAGE: &str = "stashbase/agent-sandbox:latest";

/// Named Docker volume holding the sandboxed agent's persistent home
/// directory (login state, config) across runs. Shared by every profile
/// and every run on this machine — see `docker_run_command`'s doc comment.
const PERSISTENT_HOME_VOLUME: &str = "stashbase-agent-home";

/// The container-side path `PERSISTENT_HOME_VOLUME` is mounted at, and the
/// `HOME` the sandboxed process runs with. Fixed rather than derived from
/// the host's own home directory: on Linux the container may run under an
/// arbitrary `--user uid:gid` with no passwd entry, so there's no
/// meaningful host-equivalent path to mirror.
const CONTAINER_HOME: &str = "/home/agent";

/// The default sandbox image's Dockerfile, embedded at compile time so an
/// installed `stashbase` binary can build the image itself without needing
/// this source repository on disk or a registry to pull from (neither
/// exists yet for this image).
const SANDBOX_DOCKERFILE: &str = include_str!("../../../docker/agent-sandbox/Dockerfile");

/// Where the agent container's image comes from for a given run. The
/// network-namespace holder (privileged, holds `NET_ADMIN`) always uses
/// `DEFAULT_SANDBOX_IMAGE` regardless of this choice — a custom image must
/// never run with elevated capabilities, only the unprivileged agent
/// container it's paired with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentImageSource {
    /// The built-in image, built from the embedded Dockerfile.
    Default,
    /// A pre-built image reference the caller is responsible for (`docker
    /// run` pulls it automatically if not already present locally).
    Image(String),
    /// A local Dockerfile to build. Tagged deterministically from its
    /// canonicalized path so repeated runs reuse the same build instead of
    /// rebuilding every time.
    Dockerfile(PathBuf),
}

impl AgentImageSource {
    /// Resolves an `AgentSandboxProfile`'s `image`/`dockerfile` fields (the
    /// two are mutually exclusive — enforced separately at profile
    /// validation time) into a concrete image source.
    pub(crate) fn from_profile(image: Option<&str>, dockerfile: Option<&str>) -> AgentImageSource {
        if let Some(image) = image {
            AgentImageSource::Image(image.to_owned())
        } else if let Some(dockerfile) = dockerfile {
            AgentImageSource::Dockerfile(PathBuf::from(dockerfile))
        } else {
            AgentImageSource::Default
        }
    }

    /// The tag this source's image is built/referenced under. Only
    /// meaningful for `Default`/`Dockerfile` (build targets); `Image`
    /// already names its own reference directly.
    fn build_tag(&self) -> Option<String> {
        match self {
            AgentImageSource::Default => Some(DEFAULT_SANDBOX_IMAGE.to_owned()),
            AgentImageSource::Dockerfile(path) => {
                use sha2::{Digest, Sha256};
                let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
                Some(format!(
                    "stashbase/agent-sandbox-custom:{}",
                    &hex::encode(digest)[..16]
                ))
            }
            AgentImageSource::Image(_) => None,
        }
    }

    /// The image reference `docker run` should use for the agent container.
    pub(crate) fn image_tag(&self) -> String {
        match self {
            AgentImageSource::Image(reference) => reference.clone(),
            AgentImageSource::Default | AgentImageSource::Dockerfile(_) => self
                .build_tag()
                .expect("build_tag is Some for Default and Dockerfile variants"),
        }
    }
}

/// Whether this image source's image already exists locally. A plain
/// `Image` reference is always reported as "available" — `docker run` pulls
/// it automatically if missing, the same as any ordinary `docker run
/// <image>` invocation, so there's nothing for `stashbase` itself to build.
pub(crate) fn sandbox_image_exists(source: &AgentImageSource) -> bool {
    if matches!(source, AgentImageSource::Image(_)) {
        return true;
    }
    std::process::Command::new("docker")
        .args(["image", "inspect", &source.image_tag()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Builds the image for `source` (`Default` or `Dockerfile` only — `Image`
/// has nothing to build and is rejected). Writes the Dockerfile to a
/// temporary build context directory (Docker needs a real directory to
/// build from, not stdin, so the CA-mount-style "just pass a string"
/// approach doesn't apply here) and cleans that directory up afterward
/// regardless of build outcome.
///
/// `docker build`'s own output (BuildKit's per-step progress, including
/// download/install progress for the apt and npm layers) is inherited
/// straight through to this process's stdout/stderr rather than captured —
/// the build can take a minute or more on first run (Node.js, npm
/// packages), and a silent hang would look broken. This does mean a
/// failure's error message comes from the already-visible build output,
/// not a captured string.
pub(crate) fn build_sandbox_image(source: &AgentImageSource) -> Result<(), String> {
    let tag = source
        .build_tag()
        .ok_or_else(|| "a custom image reference has nothing to build".to_owned())?;
    let build_dir =
        std::env::temp_dir().join(format!("stashbase-agent-sandbox-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&build_dir)
        .map_err(|error| format!("failed to create a temporary build directory: {error}"))?;
    let build_result = match source {
        AgentImageSource::Default => {
            std::fs::write(build_dir.join("Dockerfile"), SANDBOX_DOCKERFILE)
                .map_err(|error| format!("failed to write the embedded Dockerfile: {error}"))
                .map(|()| build_dir.clone())
        }
        AgentImageSource::Dockerfile(path) => {
            let contents = std::fs::read_to_string(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()));
            contents.and_then(|contents| {
                std::fs::write(build_dir.join("Dockerfile"), contents)
                    .map_err(|error| {
                        format!("failed to copy the Dockerfile into the build context: {error}")
                    })
                    .map(|()| build_dir.clone())
            })
        }
        AgentImageSource::Image(_) => unreachable!("checked by build_tag above"),
    }
    .and_then(|build_dir| {
        std::process::Command::new("docker")
            .args(["build", "-t", &tag])
            .arg(&build_dir)
            .status()
            .map_err(|error| format!("failed to run `docker build`: {error}"))
    })
    .and_then(|status| {
        if status.success() {
            Ok(())
        } else {
            Err("`docker build` failed; see the build output above for details".to_owned())
        }
    });
    let _ = std::fs::remove_dir_all(&build_dir);
    build_result
}

/// Builds a `docker run` invocation that mounts only the current working
/// directory (read-write), attaches the container to `network` so it can
/// reach the credential proxy at `network.gateway_ip`, and passes `env_vars`
/// explicitly via `-e` (never relying on inherited process environment,
/// since `docker run -e VAR` with no value pulls from the *calling*
/// process's environment, which would leak host env vars into the
/// container unintentionally).
///
/// Errs (fail closed) rather than building an invocation that would mount
/// an unsafe path — see `append_ca_bundle_mount`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn docker_run_command(
    command: &str,
    network: &DockerRunNetwork,
    denied_read_paths: &[String],
    denied_write_paths: &[String],
    env_vars: &std::collections::HashMap<String, String>,
    stdin_is_terminal: bool,
    agent_image: &str,
    memory_limit: Option<&str>,
    cpus_limit: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let cwd_str = cwd.to_string_lossy().into_owned();

    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        // Reusing the per-run network's name as the container's own name
        // gives the caller a deterministic handle to explicitly `docker
        // stop` this exact container during teardown, rather than relying
        // solely on `docker run`'s own SIGINT-forwarding behavior (which
        // doesn't apply to every termination path — e.g. this process
        // being killed non-gracefully) to have already stopped it.
        "--name".to_owned(),
        network.name.clone(),
        // Interactive agents (Claude Code, Cursor, Codex) need a stdin
        // stream; without `-i` the container's stdin is /dev/null and
        // every interactive TUI breaks. `-t` is only safe to add when the
        // caller's own stdin is a real terminal.
        "-i".to_owned(),
    ];
    if stdin_is_terminal {
        args.push("-t".to_owned());
    }
    args.extend([
        // The agent container itself never holds NET_ADMIN or any other
        // added capability — the network-layer egress firewall is set up
        // by a separate, short-lived helper container this one joins the
        // network namespace of (see `start_netns_holder` and the
        // `--network container:<holder>` below). Namespace rules are
        // shared by anything attached to that namespace; the capability to
        // change them is not, so this container can use the firewall but
        // never touch it.
        "--cap-drop".to_owned(),
        "ALL".to_owned(),
        "--security-opt".to_owned(),
        "no-new-privileges".to_owned(),
        // A real init process as PID 1 (Docker bundles tini for this)
        // reaps zombie processes and forwards signals correctly — without
        // it, an agent that spawns and orphans subprocesses (build tools,
        // language servers) can leak zombies for the life of the
        // container. Always on: there's no legitimate workload this could
        // break, unlike the opt-in memory/cpus limits above.
        "--init".to_owned(),
        // A generous but finite cap on the container's process count. Not
        // meant to constrain any real workload — a coding agent spawning
        // build tools, test runners, and language servers comes nowhere
        // close to this — it exists purely to contain a fork bomb (bug or
        // malicious) to the container's own cgroup instead of letting it
        // exhaust the host's PID table. Always on for the same reason
        // `--init` is: no real cost, meaningful downside blocked.
        "--pids-limit".to_owned(),
        "2048".to_owned(),
    ]);
    if cfg!(target_os = "linux") {
        // Docker Desktop already maps container-root writes on a bind
        // mount back to the host user transparently; native Linux does
        // not, so without this every file the agent creates in the
        // mounted project directory would end up root-owned.
        args.extend([
            "--user".to_owned(),
            format!("{}:{}", unsafe { libc::getuid() }, unsafe {
                libc::getgid()
            }),
        ]);
    }
    args.extend([
        "--network".to_owned(),
        format!("container:{}", netns_holder_name(network)),
    ]);
    // Opt-in only — see `AgentSandboxProfile::memory`/`cpus`. No default
    // cap: an automatic one could silently break a legitimately
    // memory/CPU-hungry task with no warning, so this only applies when a
    // profile explicitly asks for it.
    if let Some(memory) = memory_limit {
        args.extend(["--memory".to_owned(), memory.to_owned()]);
    }
    if let Some(cpus) = cpus_limit {
        args.extend(["--cpus".to_owned(), cpus.to_owned()]);
    }

    append_filesystem_mounts(&mut args, &cwd_str, denied_read_paths, denied_write_paths);
    append_ca_bundle_mount(&mut args, &cwd_str, env_vars)?;

    // A named Docker volume, not a bind mount of the real host home
    // directory, persists login/config state (e.g. Claude Code's
    // ~/.claude) across runs. Named volumes are Docker-managed storage —
    // they don't expose any other host path to the container — so this
    // doesn't reopen the filesystem allow-list `append_filesystem_mounts`
    // exists to provide. Shared across all profiles/runs by design: log
    // in once, every docker-backend run on this machine reuses it.
    args.extend([
        "-v".to_owned(),
        format!("{PERSISTENT_HOME_VOLUME}:{CONTAINER_HOME}"),
        "-e".to_owned(),
        format!("HOME={CONTAINER_HOME}"),
    ]);

    args.extend(["-w".to_owned(), cwd_str]);

    // Git identity (name/email) is not sensitive the way SSH keys or
    // credentials are, so unlike everything else outside the working
    // directory it's worth forwarding — without it, `git commit` inside
    // the container fails outright with no identity configured, since the
    // container never sees the host's real ~/.gitconfig. Env vars only
    // (not the .gitconfig file itself), so unrelated host git config
    // (aliases, signing setup pointing at host paths, etc.) doesn't leak
    // in. Caller-provided env vars win if a profile already sets one of
    // these explicitly.
    for (key, value) in host_git_identity_env_vars() {
        if !env_vars.contains_key(&key) {
            args.push("-e".to_owned());
            args.push(format!("{key}={value}"));
        }
    }

    for (key, value) in env_vars {
        args.push("-e".to_owned());
        args.push(format!("{key}={value}"));
    }
    // FORCE_COLOR is normally set on the outer `docker` process by
    // run_built_command, which has no effect on the container's own
    // environment — set it explicitly here so colored output survives.
    args.push("-e".to_owned());
    args.push("FORCE_COLOR=true".to_owned());

    // TERM/COLORTERM drive terminal-capability detection (truecolor
    // support, theme selection) in TUIs like Codex's — FORCE_COLOR alone
    // only covers basic on/off color, not that. Forwarded from the host
    // since the container has no controlling terminal of its own to
    // detect these from; caller-provided env vars still win.
    for key in ["TERM", "COLORTERM"] {
        if !env_vars.contains_key(key) {
            if let Ok(value) = std::env::var(key) {
                args.push("-e".to_owned());
                args.push(format!("{key}={value}"));
            }
        }
    }

    args.push(agent_image.to_owned());
    args.push(command.to_owned());
    Ok(("docker".to_owned(), args))
}

fn append_filesystem_mounts(
    args: &mut Vec<String>,
    cwd: &str,
    denied_read_paths: &[String],
    denied_write_paths: &[String],
) {
    let read_paths = super::subprocess::resolve_policy_paths(denied_read_paths);
    let write_paths = super::subprocess::resolve_policy_paths(denied_write_paths);

    let cwd_is_denied_write = write_paths.iter().any(|path| path == cwd);
    if cwd_is_denied_write {
        args.extend(["-v".to_owned(), format!("{cwd}:{cwd}:ro")]);
    } else {
        args.extend(["-v".to_owned(), format!("{cwd}:{cwd}")]);
    }

    for path in &read_paths {
        if !is_nested_under(path, cwd) {
            continue;
        }
        // `--tmpfs` only accepts a directory target; a file target fails
        // container creation outright ("not a directory"). Mirror the
        // native Linux bubblewrap backend's approach for a denied file:
        // bind-mount /dev/null over it read-only instead.
        if PathBuf::from(path).is_dir() {
            args.extend(["--tmpfs".to_owned(), path.clone()]);
        } else {
            args.extend(["-v".to_owned(), format!("/dev/null:{path}:ro")]);
        }
    }

    for path in &write_paths {
        if path == cwd || !is_nested_under(path, cwd) {
            continue;
        }
        if read_paths
            .iter()
            .any(|read| read == path || is_nested_under(path, read))
        {
            continue;
        }
        args.extend(["-v".to_owned(), format!("{path}:{path}:ro")]);
    }
}

/// Env vars whose value is a filesystem path to the proxy's temporary CA
/// certificate (see `Proxy::start_inner`'s `child_env` construction in
/// `proxy.rs`). The container only sees the working directory by default,
/// so these paths must be bind-mounted read-only or TLS interception
/// breaks for every tool that reads one of them to trust the proxy.
const CA_BUNDLE_ENV_KEYS: &[&str] = &[
    "SSL_CERT_FILE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "NODE_EXTRA_CA_CERTS",
    "CODEX_CA_CERTIFICATE",
];

fn host_git_config(key: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["config", "--global", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Reads the host's global git identity (`user.name`/`user.email`) and
/// maps it to the env vars git itself honors for both authoring and
/// committing. Returns an empty map if the host has neither configured —
/// this is a convenience, not a requirement, and the container works
/// fine without it (git commands that don't need an identity still run).
fn host_git_identity_env_vars() -> std::collections::HashMap<String, String> {
    let mut vars = std::collections::HashMap::new();
    if let Some(name) = host_git_config("user.name") {
        vars.insert("GIT_AUTHOR_NAME".to_owned(), name.clone());
        vars.insert("GIT_COMMITTER_NAME".to_owned(), name);
    }
    if let Some(email) = host_git_config("user.email") {
        vars.insert("GIT_AUTHOR_EMAIL".to_owned(), email.clone());
        vars.insert("GIT_COMMITTER_EMAIL".to_owned(), email);
    }
    vars
}

/// Mounts each distinct CA-bundle path found in `env_vars` into the
/// container read-only, as the single file — never its parent directory,
/// which on a typical system temp path (`/tmp/stashbase-proxy-ca-*.pem`)
/// would otherwise expose every other process's and every other agent
/// run's temp files to the container. Refuses (fails closed, per this
/// project's sandboxing policy) rather than mounting a path that is not
/// absolute or that resolves to the filesystem root — both would defeat
/// the filesystem allow-list this backend exists to provide.
fn append_ca_bundle_mount(
    args: &mut Vec<String>,
    cwd: &str,
    env_vars: &std::collections::HashMap<String, String>,
) -> Result<(), String> {
    let mut mounted_paths: Vec<String> = Vec::new();
    for key in CA_BUNDLE_ENV_KEYS {
        let Some(path) = env_vars.get(*key) else {
            continue;
        };
        if path.is_empty() || is_nested_under(path, cwd) || path == cwd {
            // Already visible through the cwd mount.
            continue;
        }
        if !PathBuf::from(path).is_absolute() {
            return Err(format!(
                "refusing to mount non-absolute CA bundle path into the Docker sandbox: {path}"
            ));
        }
        if path == "/" {
            return Err(
                "refusing to mount the filesystem root into the Docker sandbox as a CA bundle path"
                    .to_owned(),
            );
        }
        if mounted_paths.iter().any(|mounted| mounted == path) {
            continue;
        }
        args.extend(["-v".to_owned(), format!("{path}:{path}:ro")]);
        mounted_paths.push(path.clone());
    }
    Ok(())
}

fn is_nested_under(path: &str, ancestor: &str) -> bool {
    PathBuf::from(path) != PathBuf::from(ancestor)
        && PathBuf::from(path).starts_with(PathBuf::from(ancestor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// Serializes the tests that touch the real Docker daemon (image
    /// build/rebuild, container run) — `cargo test`'s default parallelism
    /// otherwise lets e.g. an image rebuild race a concurrent `docker run`
    /// of that same image, causing an intermittent "image not found" or
    /// similar transient failure that has nothing to do with the code
    /// under test.
    fn docker_daemon_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn sandbox_dockerfile_is_embedded_and_non_empty() {
        assert!(SANDBOX_DOCKERFILE.contains("FROM"));
    }

    #[test]
    fn agent_image_source_defaults_when_neither_field_is_set() {
        assert_eq!(
            AgentImageSource::from_profile(None, None),
            AgentImageSource::Default
        );
    }

    #[test]
    fn agent_image_source_prefers_image_over_dockerfile() {
        // Profile validation rejects setting both, but resolution still
        // needs a defined priority for defense in depth.
        assert_eq!(
            AgentImageSource::from_profile(Some("myorg/img:tag"), Some("./custom.Dockerfile")),
            AgentImageSource::Image("myorg/img:tag".to_owned())
        );
    }

    #[test]
    fn agent_image_source_image_tag_uses_the_reference_directly() {
        let source = AgentImageSource::from_profile(Some("myorg/img:tag"), None);
        assert_eq!(source.image_tag(), "myorg/img:tag");
    }

    #[test]
    fn agent_image_source_default_tag_is_the_builtin_image() {
        assert_eq!(AgentImageSource::Default.image_tag(), DEFAULT_SANDBOX_IMAGE);
    }

    #[test]
    fn agent_image_source_dockerfile_tag_is_deterministic_for_the_same_path() {
        let a = AgentImageSource::from_profile(None, Some("./docker/agent-sandbox/Dockerfile"));
        let b = AgentImageSource::from_profile(None, Some("./docker/agent-sandbox/Dockerfile"));
        assert_eq!(a.image_tag(), b.image_tag());
        assert!(a.image_tag().starts_with("stashbase/agent-sandbox-custom:"));
    }

    #[test]
    fn agent_image_source_dockerfile_tag_differs_for_different_paths() {
        let a = AgentImageSource::from_profile(None, Some("./docker/agent-sandbox/Dockerfile"));
        let b = AgentImageSource::from_profile(None, Some("./Cargo.toml"));
        assert_ne!(a.image_tag(), b.image_tag());
    }

    #[test]
    fn a_plain_image_reference_is_always_reported_as_already_available() {
        // No local build is possible for an image reference — `docker run`
        // pulls it automatically, same as any ordinary invocation.
        let source = AgentImageSource::Image("myorg/img:tag".to_owned());
        assert!(sandbox_image_exists(&source));
    }

    #[test]
    fn building_a_plain_image_reference_is_rejected() {
        let source = AgentImageSource::Image("myorg/img:tag".to_owned());
        let error = build_sandbox_image(&source).expect_err("nothing to build for an image ref");
        assert!(error.contains("nothing to build"));
    }

    #[test]
    fn sandbox_image_lifecycle_when_docker_available() {
        let _guard = docker_daemon_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if docker_enforcement_error().is_some() {
            eprintln!("skipping: Docker not available in this environment");
            return;
        }
        // Don't assert on the starting state — a prior test run or the
        // developer's own machine may already have the image built.
        // Just prove building it results in it existing.
        build_sandbox_image(&AgentImageSource::Default)
            .expect("building the embedded Dockerfile should succeed");
        assert!(sandbox_image_exists(&AgentImageSource::Default));
    }

    #[test]
    fn docker_binary_lookup_matches_which_docker() {
        let expected = std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| dir.join("docker").is_file())
        });
        assert_eq!(docker_binary_available(), expected);
    }

    #[test]
    fn proxy_bind_and_container_host_differ_on_docker_desktop_platforms() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let bind_host = proxy_bind_host(&network);
        let container_host = proxy_container_host(&network);
        // Docker Desktop (macOS and Windows) runs containers inside a VM,
        // so the host process can't bind to the bridge network's gateway
        // address at all — only native Linux Docker can.
        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            assert_eq!(bind_host, "127.0.0.1");
            assert_eq!(container_host, "host.docker.internal");
        } else {
            assert_eq!(bind_host, "172.30.0.1");
            assert_eq!(container_host, "172.30.0.1");
        }
    }

    #[test]
    fn rewrite_proxy_urls_replaces_only_known_proxy_keys() {
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("HTTPS_PROXY".to_owned(), "http://127.0.0.1:9999".to_owned());
        env_vars.insert("STASHBASE_GH_TOKEN".to_owned(), "127.0.0.1".to_owned());

        let rewritten =
            rewrite_proxy_urls_for_container(&env_vars, "127.0.0.1", "host.docker.internal");

        assert_eq!(rewritten["HTTPS_PROXY"], "http://host.docker.internal:9999");
        // A placeholder that happens to contain the bind host as a
        // substring must not be rewritten — only known proxy-URL keys are.
        assert_eq!(rewritten["STASHBASE_GH_TOKEN"], "127.0.0.1");
    }

    #[test]
    fn rewrite_proxy_urls_is_a_no_op_when_hosts_match() {
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert(
            "HTTPS_PROXY".to_owned(),
            "http://172.30.0.1:9999".to_owned(),
        );
        let rewritten = rewrite_proxy_urls_for_container(&env_vars, "172.30.0.1", "172.30.0.1");
        assert_eq!(rewritten, env_vars);
    }

    #[test]
    fn run_network_names_are_unique_per_call_without_a_session_id() {
        let first = generate_run_network_name(None);
        let second = generate_run_network_name(None);
        assert_ne!(first, second);
        assert!(first.starts_with("stashbase-agent-run-"));
    }

    #[test]
    fn run_network_name_uses_the_session_id_when_given() {
        // Naming the network after the run's own session id (rather than an
        // unrelated random UUID) lets a stray container/network left behind
        // after a crash be traced back to the "Agent session"/"Audit
        // session" id already shown for that run.
        let name = generate_run_network_name(Some("ags_test123"));
        assert_eq!(name, "stashbase-agent-run-ags_test123");
    }

    #[test]
    fn run_network_name_falls_back_to_a_uuid_for_an_empty_session_id() {
        let first = generate_run_network_name(Some(""));
        let second = generate_run_network_name(Some(""));
        assert_ne!(first, second);
    }

    #[test]
    fn extract_gateway_parses_docker_network_inspect_output() {
        let inspect_json =
            r#"[{"IPAM":{"Config":[{"Subnet":"172.30.0.0/16","Gateway":"172.30.0.1"}]}}]"#;
        let gateway = extract_gateway_from_inspect(inspect_json).unwrap();
        assert_eq!(gateway, "172.30.0.1");
    }

    #[test]
    fn extract_gateway_errors_on_missing_gateway() {
        let inspect_json = r#"[{"IPAM":{"Config":[{"Subnet":"172.30.0.0/16"}]}}]"#;
        assert!(extract_gateway_from_inspect(inspect_json).is_err());
    }

    #[test]
    fn create_and_remove_run_network_round_trips_when_docker_available() {
        let _guard = docker_daemon_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if docker_enforcement_error().is_some() {
            eprintln!("skipping: Docker not available in this environment");
            return;
        }
        let network = create_run_network(None).expect("network should be created");
        assert!(!network.gateway_ip.is_empty());
        remove_run_network(&network).expect("network should be removed");
    }

    #[test]
    fn list_run_networks_finds_a_network_and_extracts_its_session_id_when_docker_available() {
        let _guard = docker_daemon_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if docker_enforcement_error().is_some() {
            eprintln!("skipping: Docker not available in this environment");
            return;
        }
        let session_id = format!("ags_listtest{}", uuid::Uuid::new_v4().simple());
        let network = create_run_network(Some(&session_id)).expect("network should be created");

        let found = list_run_networks()
            .expect("listing should succeed")
            .into_iter()
            .find(|entry| entry.name == network.name);
        let entry = found.expect("the just-created network should be in the listing");
        assert_eq!(entry.session_id.as_deref(), Some(session_id.as_str()));
        assert!(!entry.created_at.is_empty());

        remove_run_network(&network).expect("network should be removed");
        let still_listed = list_run_networks()
            .expect("listing should succeed")
            .into_iter()
            .any(|entry| entry.name == network.name);
        assert!(!still_listed, "removed network should no longer be listed");
    }

    #[test]
    fn remove_run_network_succeeds_even_with_a_still_running_container() {
        let _guard = docker_daemon_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if docker_enforcement_error().is_some() {
            eprintln!("skipping: Docker not available in this environment");
            return;
        }
        let network = create_run_network(None).expect("network should be created");
        // Start a long-running container on this network with the same
        // name `docker_run_command` would give it, without `--rm`, so it
        // is still attached when teardown runs — reproducing the "network
        // has active endpoints" failure this function exists to avoid.
        let start = std::process::Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &network.name,
                "--network",
                &network.name,
                DEFAULT_SANDBOX_IMAGE,
                "sleep",
                "300",
            ])
            .output()
            .expect("docker run should execute");
        assert!(
            start.status.success(),
            "failed to start test container: {}",
            String::from_utf8_lossy(&start.stderr)
        );
        remove_run_network(&network)
            .expect("network removal should succeed by stopping the still-running container first");
    }

    #[test]
    fn netns_holder_name_is_derived_from_network_name() {
        let network = DockerRunNetwork {
            name: "stashbase-agent-run-abc123".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        assert_eq!(
            netns_holder_name(&network),
            "stashbase-agent-run-abc123-netns-holder"
        );
    }

    #[test]
    fn parse_proxy_host_port_reads_https_proxy() {
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert(
            "HTTPS_PROXY".to_owned(),
            "http://host.docker.internal:54321".to_owned(),
        );
        let (host, port) = parse_proxy_host_port(&env_vars).unwrap();
        assert_eq!(host, "host.docker.internal");
        assert_eq!(port, "54321");
    }

    #[test]
    fn parse_proxy_host_port_falls_back_to_http_proxy() {
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("HTTP_PROXY".to_owned(), "http://172.17.0.1:9999".to_owned());
        let (host, port) = parse_proxy_host_port(&env_vars).unwrap();
        assert_eq!(host, "172.17.0.1");
        assert_eq!(port, "9999");
    }

    #[test]
    fn parse_proxy_host_port_returns_none_when_absent() {
        let env_vars = std::collections::HashMap::new();
        assert!(parse_proxy_host_port(&env_vars).is_none());
    }

    #[test]
    fn netns_holder_blocks_direct_egress_but_allows_proxy_when_docker_available() {
        let _guard = docker_daemon_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if docker_enforcement_error().is_some() {
            eprintln!("skipping: Docker not available in this environment");
            return;
        }
        let network = create_run_network(None).expect("network should be created");

        // A tiny host-side listener the holder's firewall rule should allow
        // through (simulating the credential proxy).
        // Short name: DNS labels cap out at 63 characters, and
        // `network.name` (already `stashbase-agent-run-<uuid>`) plus a
        // suffix would exceed that, making it unresolvable — a test
        // artifact only, real usage never resolves container names.
        let listener_container = format!("listener-{}", uuid::Uuid::new_v4().simple());
        let listen = std::process::Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &listener_container,
                "--network",
                &network.name,
                DEFAULT_SANDBOX_IMAGE,
                "node",
                "-e",
                "require('http').createServer((_, response) => response.end('ok')).listen(18234)",
            ])
            .output()
            .expect("docker run should execute");
        assert!(listen.status.success());
        // Give the listener a moment to bind before anything tries to
        // reach it.
        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert(
            "HTTPS_PROXY".to_owned(),
            format!("http://{listener_container}:18234"),
        );
        let holder_result = start_netns_holder(&network, &env_vars);
        let cleanup = || {
            stop_container_if_running(&listener_container);
            let _ = remove_run_network(&network);
        };
        if let Err(error) = holder_result {
            cleanup();
            panic!("start_netns_holder failed: {error}");
        }

        let holder_name = netns_holder_name(&network);
        let allowed = std::process::Command::new("docker")
            .args([
                "exec",
                &holder_name,
                "curl",
                "-s",
                "-m",
                "5",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &format!("http://{listener_container}:18234/"),
            ])
            .output();
        let blocked = std::process::Command::new("docker")
            .args([
                "exec",
                &holder_name,
                "curl",
                "-s",
                "-m",
                "5",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "https://example.com",
            ])
            .output();
        // DNS must not be a data-exfiltration side channel: Docker's
        // embedded resolver forwards unresolved lookups via the host's own
        // DNS stack, entirely outside this container's network namespace
        // — no iptables rule inside the container can see, let alone
        // block, that traffic. The only real fix is disabling upstream
        // forwarding at the source (`--dns 0.0.0.0` on the holder, which
        // the agent inherits) — assert that's actually working, not just
        // that the firewall rule exists.
        let dns_lookup = std::process::Command::new("docker")
            .args(["exec", &holder_name, "getent", "hosts", "example.com"])
            .output();

        cleanup();

        let allowed = allowed.expect("docker exec should run");
        let blocked = blocked.expect("docker exec should run");
        let dns_lookup = dns_lookup.expect("docker exec should run");
        assert_eq!(
            String::from_utf8_lossy(&allowed.stdout),
            "200",
            "the proxy-equivalent listener should be reachable through the firewall"
        );
        assert_eq!(
            String::from_utf8_lossy(&blocked.stdout),
            "000",
            "a direct request to an arbitrary host should be blocked by the firewall"
        );
        assert!(
            !dns_lookup.status.success(),
            "an external hostname must not resolve at all — a successful lookup here means \
             Docker's embedded DNS resolver is still forwarding queries upstream, which is a \
             data-exfiltration channel no container-level firewall rule can block"
        );
    }

    #[test]
    fn docker_run_command_mounts_cwd_and_sets_env_with_no_denied_paths() {
        let network = DockerRunNetwork {
            name: "test-network".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert(
            "HTTPS_PROXY".to_owned(),
            "https://172.30.0.1:9999".to_owned(),
        );

        let (program, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &env_vars,
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();

        assert_eq!(program, "docker");
        assert!(args.contains(&"run".to_owned()));
        assert!(args.contains(&"--rm".to_owned()));
        assert!(args.contains(&"--network".to_owned()));
        assert!(args.contains(&"test-network".to_owned()));
        assert!(args.contains(&"-e".to_owned()));
        assert!(args.contains(&"HTTPS_PROXY=https://172.30.0.1:9999".to_owned()));
        assert!(args.contains(&DEFAULT_SANDBOX_IMAGE.to_owned()));
        assert_eq!(args[args.len() - 2], DEFAULT_SANDBOX_IMAGE);
        assert_eq!(args[args.len() - 1], "claude");
    }

    #[test]
    fn docker_run_command_mounts_cwd_readwrite_when_no_deny_paths() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(args.contains(&"-v".to_owned()));
        let mount_index = args.iter().position(|arg| arg == "-v").unwrap();
        assert_eq!(args[mount_index + 1], format!("{cwd}:{cwd}"));
    }

    #[test]
    fn docker_run_command_shadow_mounts_nested_deny_read_path_as_tmpfs() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let cwd = std::env::current_dir().unwrap();
        let nested = cwd.join(".git").to_string_lossy().into_owned();
        let (_, args) = docker_run_command(
            "claude",
            &network,
            std::slice::from_ref(&nested),
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&"--tmpfs".to_owned()));
        let tmpfs_index = args.iter().position(|arg| arg == "--tmpfs").unwrap();
        assert_eq!(args[tmpfs_index + 1], nested);
    }

    #[test]
    fn docker_run_command_shadow_mounts_nested_deny_read_file_as_dev_null_bind() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let cwd = std::env::current_dir().unwrap();
        // Cargo.toml is a real file (not a directory) inside this repo's
        // cwd — `--tmpfs` on a file target fails container creation
        // outright ("not a directory"), so a denied file must be shadowed
        // with a read-only /dev/null bind mount instead.
        let nested = cwd.join("Cargo.toml").to_string_lossy().into_owned();
        let (_, args) = docker_run_command(
            "claude",
            &network,
            std::slice::from_ref(&nested),
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(!args.contains(&"--tmpfs".to_owned()));
        let mount = args
            .windows(2)
            .find(|pair| pair[0] == "-v" && pair[1] == format!("/dev/null:{nested}:ro"))
            .unwrap_or_else(|| panic!("expected a /dev/null bind mount for the denied file"));
        assert_eq!(mount[1], format!("/dev/null:{nested}:ro"));
    }

    #[test]
    fn docker_run_command_shadow_mounts_nested_deny_write_path_readonly() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let cwd = std::env::current_dir().unwrap();
        let nested = cwd.join("Cargo.lock").to_string_lossy().into_owned();
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            std::slice::from_ref(&nested),
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        let nested_mount = args
            .windows(2)
            .find(|pair| pair[0] == "-v" && pair[1].starts_with(&format!("{nested}:")))
            .unwrap_or_else(|| panic!("no -v mount found for nested path {nested}"));
        assert_eq!(nested_mount[1], format!("{nested}:{nested}:ro"));
    }

    #[test]
    fn docker_run_command_mounts_cwd_readonly_when_cwd_itself_is_denied_write() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            std::slice::from_ref(&cwd),
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        let mount_index = args.iter().position(|arg| arg == "-v").unwrap();
        assert_eq!(args[mount_index + 1], format!("{cwd}:{cwd}:ro"));
        // The cwd mount plus the persistent home volume mount — no extra
        // shadow mount, since the cwd mount itself is already read-only.
        assert_eq!(args.iter().filter(|arg| *arg == "-v").count(), 2);
    }

    #[test]
    fn docker_run_command_forwards_host_git_identity_when_configured() {
        let Some(name) = host_git_config("user.name") else {
            eprintln!("skipping: host has no global git user.name configured");
            return;
        };
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&format!("GIT_AUTHOR_NAME={name}")));
        assert!(args.contains(&format!("GIT_COMMITTER_NAME={name}")));
    }

    #[test]
    fn docker_run_command_lets_caller_env_vars_override_host_git_identity() {
        if host_git_config("user.name").is_none() {
            eprintln!("skipping: host has no global git user.name configured");
            return;
        }
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("GIT_AUTHOR_NAME".to_owned(), "Explicit Override".to_owned());
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &env_vars,
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&"GIT_AUTHOR_NAME=Explicit Override".to_owned()));
        assert_eq!(
            args.iter()
                .filter(|arg| arg.starts_with("GIT_AUTHOR_NAME="))
                .count(),
            1
        );
    }

    #[test]
    fn docker_run_command_mounts_ca_bundle_file_not_its_directory() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert(
            "SSL_CERT_FILE".to_owned(),
            "/tmp/stashbase-ca/ca.pem".to_owned(),
        );
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &env_vars,
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        // Must mount only the exact file — mounting its parent directory
        // would expose every other file in it (other processes' temp
        // files, other agent runs' audit/revocation state) to the
        // container, defeating the filesystem allow-list.
        assert!(args.contains(&"/tmp/stashbase-ca/ca.pem:/tmp/stashbase-ca/ca.pem:ro".to_owned()));
        assert!(!args
            .iter()
            .any(|arg| arg == "/tmp/stashbase-ca:/tmp/stashbase-ca:ro"));
    }

    #[test]
    fn docker_run_command_refuses_ca_bundle_path_at_filesystem_root() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("SSL_CERT_FILE".to_owned(), "/".to_owned());
        let result = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &env_vars,
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn docker_run_command_refuses_non_absolute_ca_bundle_path() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("SSL_CERT_FILE".to_owned(), "ca.pem".to_owned());
        let result = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &env_vars,
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn docker_run_command_names_the_container_after_the_network() {
        let network = DockerRunNetwork {
            name: "stashbase-agent-run-some-uuid".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        let name_index = args.iter().position(|arg| arg == "--name").unwrap();
        assert_eq!(args[name_index + 1], network.name);
    }

    #[test]
    fn docker_run_command_always_passes_interactive_stdin_flag() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&"-i".to_owned()));
        assert!(!args.contains(&"-t".to_owned()));
    }

    #[test]
    fn docker_run_command_adds_pty_flag_only_when_stdin_is_a_terminal() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            true,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&"-t".to_owned()));
    }

    #[test]
    fn docker_run_command_mounts_persistent_home_volume_and_sets_home() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&format!("{PERSISTENT_HOME_VOLUME}:{CONTAINER_HOME}")));
        assert!(args.contains(&format!("HOME={CONTAINER_HOME}")));
    }

    #[test]
    fn docker_run_command_drops_capabilities_and_denies_new_privileges() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        let cap_drop_index = args.iter().position(|arg| arg == "--cap-drop").unwrap();
        assert_eq!(args[cap_drop_index + 1], "ALL");
        // No --cap-add: the agent container never holds any added
        // capability. The network-layer firewall is set up by a separate
        // helper container (see start_netns_holder) whose namespace this
        // one joins via `--network container:<holder>`.
        assert!(!args.contains(&"--cap-add".to_owned()));
        assert!(args.contains(&"--security-opt".to_owned()));
        assert!(args.contains(&"no-new-privileges".to_owned()));
    }

    #[test]
    fn docker_run_command_always_adds_init_and_a_pids_limit() {
        // Unlike memory/cpus, these are never configurable per profile and
        // always applied — there's no legitimate workload either could
        // break, only a fork bomb or zombie-process leak they exist to
        // contain.
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(args.contains(&"--init".to_owned()));
        let pids_limit_index = args.iter().position(|arg| arg == "--pids-limit").unwrap();
        assert_eq!(args[pids_limit_index + 1], "2048");
    }

    #[test]
    fn docker_run_command_omits_resource_limits_by_default() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        assert!(!args.contains(&"--memory".to_owned()));
        assert!(!args.contains(&"--cpus".to_owned()));
    }

    #[test]
    fn docker_run_command_adds_memory_and_cpu_limits_when_configured() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            Some("2g"),
            Some("1.5"),
        )
        .unwrap();
        let memory_index = args.iter().position(|arg| arg == "--memory").unwrap();
        assert_eq!(args[memory_index + 1], "2g");
        let cpus_index = args.iter().position(|arg| arg == "--cpus").unwrap();
        assert_eq!(args[cpus_index + 1], "1.5");
    }

    #[test]
    fn docker_run_command_joins_the_netns_holders_network() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        let network_index = args.iter().position(|arg| arg == "--network").unwrap();
        assert_eq!(args[network_index + 1], "container:n-netns-holder");
    }

    #[test]
    fn docker_run_command_uses_user_flag_only_on_linux() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &std::collections::HashMap::new(),
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        // Restoring `--user` is safe here: the agent container never runs
        // any privileged setup itself, so it can start as the target
        // uid/gid immediately, unlike the earlier setpriv-based approach.
        if cfg!(target_os = "linux") {
            assert!(args.contains(&"--user".to_owned()));
        }
    }

    #[test]
    fn network_create_args_add_internal_flag_only_on_linux() {
        let args = network_create_args("n");
        if cfg!(target_os = "linux") {
            assert!(args.iter().any(|arg| arg == "--internal"));
        } else {
            assert!(!args.iter().any(|arg| arg == "--internal"));
        }
    }

    #[test]
    fn docker_run_command_skips_ca_bundle_mount_when_already_under_cwd() {
        let network = DockerRunNetwork {
            name: "n".to_owned(),
            gateway_ip: "172.30.0.1".to_owned(),
        };
        let cwd = std::env::current_dir().unwrap();
        let ca_path = cwd.join("ca.pem").to_string_lossy().into_owned();
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("SSL_CERT_FILE".to_owned(), ca_path);
        let (_, args) = docker_run_command(
            "claude",
            &network,
            &[],
            &[],
            &env_vars,
            false,
            DEFAULT_SANDBOX_IMAGE,
            None,
            None,
        )
        .unwrap();
        // Only the cwd mount and the persistent home volume mount should
        // exist; no extra mount for a CA path that's already inside the
        // working directory.
        assert_eq!(args.iter().filter(|arg| *arg == "-v").count(), 2);
    }
}
