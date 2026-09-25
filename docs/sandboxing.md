# Sandboxing

`stashbase agent run` isolates the child process's filesystem and network access using one of two backends. Both are configured per profile under `[sandbox]` and `[filesystem]` — see [Agent Profiles](agent-profiles.md) for the rest of the profile schema (secrets, egress rules, MCP tool restrictions, etc.).

**Recommendation: use the Docker backend whenever Docker is available.** It's meaningfully stronger on every axis that matters for running an agent you don't fully trust:

- **Filesystem**: allow-list (only the working directory is visible at all) instead of deny-list (specific paths blocked, everything else still reachable).
- **Network**: enforced by a real firewall inside the container's network namespace, not by the agent choosing to honor `HTTPS_PROXY`/`HTTP_PROXY` — a process that deliberately opens a raw socket is blocked the same as one that respects the proxy.
- **Platform coverage**: works identically on macOS, Linux, and Windows (via Docker Desktop), rather than the native backend's platform-specific mechanisms that don't exist on Windows at all.
- **Extensibility**: `sandbox.image`/`sandbox.dockerfile` let a profile add exactly the tools it needs (Python, a compiler, whatever) without weakening the sandbox itself.

The native backend stays the default because it needs nothing beyond the CLI itself — no Docker install, no daemon, no image to build — which matters for a quick first run. But once Docker is available, there's no real reason to prefer the weaker guarantees of the native backend over it.

## Native backend (default)

No configuration needed — this is what every profile gets unless `[sandbox] backend = "docker"` is set. Filesystem restrictions are opt-in:

```toml
[filesystem]
deny_read = ["~/.ssh", "~/.aws"]
deny_write = ["~/.git"]
```

Paths use explicit prefixes: `~` for home, relative paths for the current directory. Enforcement uses platform-native mechanisms:
- **macOS**: Seatbelt sandbox
- **Linux**: `systemd-run` or `bubblewrap` (automatic fallback)
- **Windows and other unsupported platforms**: Validation fails closed; the run does not proceed with the native backend

Existing file descriptors and data already in process memory remain unrestricted. Network egress is still enforced the same way it is under the Docker backend — through the loopback credential proxy and `egress_hosts`/`deny_hosts` — but there is no network-layer firewall backing that up the way there is for Docker; a process that ignores `HTTPS_PROXY`/`HTTP_PROXY` entirely and opens a raw connection can reach the network directly under the native backend.

**Windows users**: the native backend doesn't support Windows at all, but the Docker backend does — it only needs Docker Desktop, not any platform-native sandboxing primitive. `agent validate` correctly checks Docker readiness instead of the native mechanisms for a Docker-backend profile, so it won't falsely report Windows as unsupported for a profile that sets `backend = "docker"`.

## Docker backend

Opt into container-based isolation instead:

```toml
[sandbox]
backend = "docker"
```

Or override the profile's choice for a single invocation without editing the file, with `--docker-sandbox`:

```bash
stashbase agent run --profile coding --docker-sandbox true -- claude   # force Docker for this run
stashbase agent run --profile coding --docker-sandbox false -- claude  # force native for this run
```

`--docker-sandbox` only overrides which backend enforces the run; it does not change any other profile setting (secrets, egress rules, `deny_read`/`deny_write`, etc.). Omit it to use whatever the profile declares.

With `backend = "docker"`, the agent process runs inside a container on a fresh, isolated Docker network created for that single `agent run` invocation. Compared to the native backend:

- The container has its own network namespace and can reach the host's credential proxy but nothing else — no LAN, no other local processes, no host-only services. This is enforced at the network layer inside the container (an `iptables` rule described below), not merely by the agent choosing to honor `HTTPS_PROXY`/`HTTP_PROXY`: a process that deliberately ignores those env vars and opens a raw connection is blocked the same as one that respects them, on both macOS and Linux.
- Filesystem access is allow-list, not deny-list: only the current working directory is visible inside the container. `deny_read`/`deny_write` paths outside the working directory are already invisible; paths inside it are additionally shadow-mounted (empty for `deny_read`, read-only for `deny_write`) so the same guarantee holds.
- Requires Docker installed and the daemon running. If Docker isn't available, the run fails closed with an error — it does not fall back to running unsandboxed or to the native backend.
- On Docker Desktop (macOS/Windows), the proxy binds to loopback and the container reaches it via `host.docker.internal`, since Desktop containers run inside a VM and cannot reach the host's bridge-network gateway directly. On native Linux Docker, the proxy binds to the per-run network's gateway address instead. Both platforms additionally get the network-layer firewall rule described below, which is what actually blocks a bypass attempt — the platform difference here only affects how the container reaches the proxy, not whether egress is enforced.

### How network egress is enforced

Enforcement is split across two containers per run, not built into the agent container itself:

1. A short-lived **network namespace holder** is started first, attached to the run's isolated network, holding the `NET_ADMIN` Linux capability (`--cap-drop ALL --cap-add NET_ADMIN`). It installs one `iptables` rule — default-DROP all outbound traffic, with exceptions only for loopback, DNS, and the credential proxy's specific resolved address and port — then verifies the rule actually took effect (a known-arbitrary host must be unreachable, and the proxy itself must still be reachable; either check failing means setup fails closed rather than continuing with a possibly-ineffective firewall) before blocking forever, keeping that network namespace alive.
2. The **agent container** joins that exact network namespace (`--network container:<holder>`) but holds no added capabilities of its own at all (`--cap-drop ALL`, nothing re-added). Namespace rules — including the firewall — are shared by anything attached to the namespace; the *capability* to change them is not. The agent container can use the firewall but can never modify it.

This two-container split exists because the more obvious approach — start the agent container as root with `NET_ADMIN`, set up the firewall, then drop privileges and capabilities before running the real command — turned out not to work on Docker Desktop: dropping capabilities from inside a container requires the `CAP_SETPCAP` capability, and Docker Desktop silently zeroes out a container's entire capability set the moment `CAP_SETPCAP` is requested (confirmed directly, not assumed). Splitting the privileged setup into a separate container that never runs the actual agent sidesteps this entirely — the agent container never needs `CAP_SETPCAP`, `NET_ADMIN`, or root, on any platform.

This was verified directly, including trying to defeat it from inside a real sandboxed session: a deliberate bypass attempt (unsetting all proxy env vars and issuing a raw `curl` to an arbitrary host) is blocked with a connection failure, identical to what the credential proxy itself returns for a denied host — and an attempt to run `iptables -F OUTPUT` from inside the agent container to erase the rule and reopen egress fails outright with "Permission denied," since the agent process holds zero capabilities.

### Supported agents and the sandbox image

Claude Code and Codex are pre-installed in the default sandbox image and are the only agents validated against this backend so far. Other tools that don't need anything beyond what the image provides should also run.

The default image is built from `node:22-bookworm-slim` (Debian underneath) with `git`, `curl`, `ca-certificates`, `bubblewrap`, `iptables`, `jq`, `gh`, `dnsutils`, `unzip`, `less`, `procps`, and Python 3 (`python3`, `python3-pip`, `python3-venv`) installed via `apt`, plus Claude Code and Codex installed via their own official native installer scripts (not `npm install -g` — see the Dockerfile for why). `pip install` works out of the box without needing a virtualenv first — Debian's system pip normally refuses this (PEP 668), but that protection matters less for an ephemeral sandbox container than a real host, so it's relaxed here. It is not published to a registry — the Dockerfile is embedded in the `stashbase` binary itself, so a plain installed copy of the CLI can build it locally without needing this source repository. The first `agent run` that selects the Docker backend detects the image is missing and offers to build it (interactively; `--silent` runs fail closed instead of prompting). The build streams Docker's own progress live rather than running silently.

### Custom images

A profile can run a different image instead of the built-in default, either your own pre-built image or a custom Dockerfile — for example to add a language toolchain, a package manager, or other tools your agent needs. The default image stays the maintained baseline for everyone; this is the escape hatch for when it isn't enough.

```toml
[sandbox]
backend = "docker"
image = "myorg/my-agent-image:latest"   # a pre-built image; `docker run` pulls it if missing locally
```

```toml
[sandbox]
backend = "docker"
dockerfile = "./sandbox.Dockerfile"     # path relative to the current working directory; stashbase builds and tags it locally
```

`image` and `dockerfile` are mutually exclusive — set at most one. Neither weakens the sandbox itself: regardless of which image runs, the agent container always gets `--cap-drop ALL`, `no-new-privileges`, and the same network-namespace-holder firewall described above — a custom image can add tools, but it cannot request more capabilities or opt out of network/filesystem enforcement. The network-namespace holder itself (the one privileged container, holding `NET_ADMIN`) always uses the built-in default image regardless of this setting, never a custom one.

A `dockerfile` build is tagged deterministically from its path and only rebuilt when that tag doesn't already exist locally — edit the Dockerfile and remove the old image (`docker image rm`) to force a rebuild.

A minimal base image needs `ca-certificates` installed for TLS-intercepted HTTPS requests to work — without it, the image's HTTP client can't validate the proxy's injected CA and HTTPS calls fail with a certificate error even though the request itself was allowed by policy. Verified with a plain `alpine:latest` image: unencrypted HTTP calls to allowed and denied hosts behave correctly out of the box, but HTTPS needs `apk add ca-certificates` (or the base image's equivalent) first. A musl-based image like Alpine also needs a glibc compatibility shim (e.g. `apk add gcompat libstdc++`) to run Claude Code's or Codex's native installer binaries, which are built against glibc — verified working with this combination.

`--docker-image <ref>` and `--docker-dockerfile <path>` override `sandbox.image`/`sandbox.dockerfile` for a single invocation, the same way `--docker-sandbox` overrides `sandbox.backend` — useful for trying a different image without editing the profile file. Either flag implies the Docker backend for that run even if the profile declares `backend = "native"` (or doesn't set `[sandbox]` at all), and the two are mutually exclusive with each other:

```bash
stashbase agent run --profile coding --docker-image node:22-alpine -- claude
```

### Git identity

Your global `git config user.name` and `user.email` (if configured on the host) are forwarded into the container as `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, and `GIT_COMMITTER_EMAIL`. This is the one piece of host configuration deliberately forwarded despite the filesystem allow-list, since it's authorship metadata, not a credential — without it, `git commit` inside the sandbox fails with no identity configured. It does not grant push access: `git push` (or any other authenticated git operation) still needs a real credential, wired through `[secrets]` like `GITHUB_TOKEN`, or run from outside the sandbox. Raw SSH keys are never forwarded. A profile that explicitly sets one of these four env vars itself takes precedence over the forwarded host value.

### Login persistence

Agent login/config state (e.g. Claude Code's `~/.claude`, Codex's `~/.codex`) is kept in a Docker-managed named volume, not a bind mount of your real home directory, so it survives across `agent run` invocations without exposing anything else on the host. This volume is shared across every profile, project, *and image* using the Docker backend on this machine — logging in once covers all of them, even after switching to a completely different custom image or Dockerfile, since the volume is mounted at the same container path (`/home/agent`) regardless of which image runs.

### Codex and subscription login

Codex's normal OAuth login flow opens a browser that redirects to a local HTTP callback server. That callback listens inside the container's own network namespace, which the host browser cannot reach — the container's `localhost` is not your machine's `localhost`. Use Codex's device-code flow instead, which doesn't depend on a local callback at all:

```bash
stashbase agent run --profile coding -- codex login --device-auth
```

`auth.openai.com` must be in `egress_hosts` for device-code login (and its silent token refresh) to work — it's a different host than `api.openai.com`, which only serves completions.

Claude Code has the same kind of gap: `platform.claude.com` must be in `egress_hosts` alongside `api.anthropic.com` for OAuth login (`/login`) and silent token refresh to work. Without it, login fails with "OAuth error: proxy refused the connection," or — if you were already logged in before restricting egress — the session works until the access token's next refresh is silently blocked, then fails hours later with "OAuth access token has expired."

### Cleaning up after a crash

Every per-run Docker network (and its two containers) is named after that run's own session id — the same `ags_...` id shown in "Agent session"/"Audit session" and used for the audit log filename — specifically so leftovers can be traced back to the run that created them. Normal exit paths, including Ctrl+C, tear both containers and the network down as part of `agent run` itself; only a crash or a forceful `SIGKILL` of the `stashbase` process can leave them behind.

```bash
stashbase agent docker cleanup
```

Lists any `stashbase-agent-run-*` networks still present, skips ones tied to a session this machine still has a live local record for, and asks for confirmation before removing the rest (`--yes` skips the prompt). A `--remote` run has no local record to check against at all, so a listed network could in principle still belong to a remote session genuinely in progress — the command shows each one's creation time so you can judge, rather than guessing on your behalf.

To just look without removing anything:

```bash
stashbase agent docker status
```

Lists the same networks (name, session id, creation time, and whether it's tied to a live local session) — pass `--json` for machine-readable output.

### Managing the default image

```bash
stashbase agent docker build [--force]
```

Builds the default sandbox image ahead of time instead of waiting to be prompted on first `agent run`, or rebuilds it with `--force` (e.g. after the embedded Dockerfile picks up new apt packages or a security patch) without needing to `docker rmi` it by hand first.

Add `--profile <name>` to target that profile's own `sandbox.image`/`sandbox.dockerfile` instead of the default — useful for pre-building or force-refreshing a custom image the same way, without needing to trigger a real `agent run` first. `--profile-source auto|global|directory` controls where `--profile` is loaded from, same as `agent run`/`agent validate`. A profile using a plain `image` reference has nothing to build (`docker run` pulls it automatically), so this reports that and does nothing rather than erroring.

### Checking readiness

```bash
stashbase agent docker doctor
```

Checks whether the Docker sandbox backend can actually run here — the `docker` CLI on PATH, the daemon reachable (and its version), and whether the default image is already built — without starting a real sandboxed run to find out. Exits non-zero if anything's not ready; pass `--json` for machine-readable output. Useful for onboarding or CI setup scripts that want to fail fast with a clear reason, rather than discovering a missing Docker install only when a real `agent run` fails.

### Docker backend limitations

- Two containers run per invocation (the network namespace holder plus the agent container itself), not one — slightly more setup overhead per run than a single-container approach, in exchange for the firewall being enforced by capability separation rather than a privilege drop inside the agent container.
- The persistent home volume is shared across every profile and project — chat history and config from one profile's sandboxed sessions are visible to another profile's sandboxed sessions on the same machine. This is a privacy boundary, not a security one: it never grants access beyond what each run's own profile allows, since egress/credential policy is enforced per-run regardless of what's in the shared volume.

This backend is early access, opt-in only, and does not change the default behavior of existing profiles.
