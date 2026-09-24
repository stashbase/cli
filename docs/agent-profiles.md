# Agent Profiles

Agent profiles configure secure, credential-restricted environments for running CLI tools and agents. Instead of exposing actual API keys to child processes, Stashbase provides short-lived placeholders. An HTTP proxy enforces access policies—deciding which hosts, HTTP methods, and URL paths may receive each credential.

> **Early access — local exposure reduction, not hostile-agent isolation.**
> Agent profiles keep granted secrets out of child processes and inject them only through the HTTP proxy. They do not prevent a malicious same-user process from accessing broader Stashbase credentials or bypassing this workflow.

## Profiles: Local vs. Remote

**Local Agent Proxy sessions** run a short-lived proxy on your machine. Secrets are resolved in an ephemeral process, and the child sees only opaque placeholders. Useful for local development and testing.

**Remote Agent Proxy sessions** (`--remote` flag) resolve secrets on Stashbase infrastructure (requires active plan). The CLI creates a scoped session token that exists only in memory and is revoked when the child exits. Personal credentials are fully private—never fetched, printed, or stored by the CLI. Perfect for CI/CD and shared environments.

## Quick Start

Create a profile template:

```bash
stashbase agent init my-profile
```

This creates `.stashbase/agents/my-profile.toml` with placeholder sections. Edit it to configure your secrets and policies, then run:

```bash
stashbase agent run --profile my-profile -- <your-command>
```

For a remote session:

```bash
stashbase agent run --remote --profile my-profile -- <your-command>
```

## Simple Profile Example

```toml
# .stashbase/agents/basic.toml
egress_hosts = ["api.github.com", "registry.npmjs.org"]

[secrets]
project = "my-project"
environment = "development"

[secrets.GITHUB_TOKEN]
env = "GH_TOKEN"

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/user", "/repos/*"]
```

The child process receives `GH_TOKEN` as a placeholder. When it makes a matching HTTP request (GET to `/user` or `/repos/*` on `api.github.com`), the proxy injects the real token. Any unmatched request is blocked.

## Filesystem and Network Restrictions

Restrict what the child can read or write:

```toml
[filesystem]
deny_read = ["~/.ssh", "~/.aws"]
deny_write = ["~/.git"]
```

Paths use explicit prefixes: `~` for home, relative paths for the current directory. Enforcement uses platform-native mechanisms:
- **macOS**: Seatbelt sandbox
- **Linux**: `systemd-run` or `bubblewrap` (automatic fallback)
- **Unsupported platforms**: Validation fails closed; the run does not proceed

Existing file descriptors and data already in process memory remain unrestricted.

## Sandbox Backend

By default, filesystem/network enforcement uses the platform-native mechanism described above. Opt into Docker-based isolation instead:

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

- The container has its own network namespace and can reach the host's credential proxy but nothing else — no LAN, no other local processes, no host-only services. This is enforced at the network layer inside the container (an `iptables` rule the entrypoint installs before running the agent — see below), not merely by the agent choosing to honor `HTTPS_PROXY`/`HTTP_PROXY`: a process that deliberately ignores those env vars and opens a raw connection is blocked the same as one that respects them, on both macOS and Linux.
- Filesystem access is allow-list, not deny-list: only the current working directory is visible inside the container. `deny_read`/`deny_write` paths outside the working directory are already invisible; paths inside it are additionally shadow-mounted (empty for `deny_read`, read-only for `deny_write`) so the same guarantee holds.
- Requires Docker installed and the daemon running. If Docker isn't available, the run fails closed with an error — it does not fall back to running unsandboxed or to the native backend.
- On Docker Desktop (macOS/Windows), the proxy binds to loopback and the container reaches it via `host.docker.internal`, since Desktop containers run inside a VM and cannot reach the host's bridge-network gateway directly. On native Linux Docker, the proxy binds to the per-run network's gateway address instead. Both platforms additionally get the network-layer firewall rule described above, which is what actually blocks a bypass attempt — the platform difference here only affects how the container reaches the proxy, not whether egress is enforced.

### How network egress is enforced

Enforcement is split across two containers per run, not built into the agent container itself:

1. A short-lived **network namespace holder** is started first, attached to the run's isolated network, holding the `NET_ADMIN` Linux capability (`--cap-drop ALL --cap-add NET_ADMIN`). It installs one `iptables` rule — default-DROP all outbound traffic, with exceptions only for loopback, DNS, and the credential proxy's specific resolved address and port — then blocks forever, keeping that network namespace alive.
2. The **agent container** joins that exact network namespace (`--network container:<holder>`) but holds no added capabilities of its own at all (`--cap-drop ALL`, nothing re-added). Namespace rules — including the firewall — are shared by anything attached to the namespace; the *capability* to change them is not. The agent container can use the firewall but can never modify it.

This two-container split exists because the more obvious approach — start the agent container as root with `NET_ADMIN`, set up the firewall, then drop privileges and capabilities before running the real command — turned out not to work on Docker Desktop: dropping capabilities from inside a container requires the `CAP_SETPCAP` capability, and Docker Desktop silently zeroes out a container's entire capability set the moment `CAP_SETPCAP` is requested (confirmed directly, not assumed). Splitting the privileged setup into a separate container that never runs the actual agent sidesteps this entirely — the agent container never needs `CAP_SETPCAP`, `NET_ADMIN`, or root, on any platform.

This was verified directly, including trying to defeat it from inside a real sandboxed session: a deliberate bypass attempt (unsetting all proxy env vars and issuing a raw `curl` to an arbitrary host) is blocked with a connection failure, identical to what the credential proxy itself returns for a denied host — and an attempt to run `iptables -F OUTPUT` from inside the agent container to erase the rule and reopen egress fails outright with "Permission denied," since the agent process holds zero capabilities.

### Supported agents and the sandbox image

Claude Code and Codex are pre-installed in the sandbox image and are the only agents validated against this backend so far. Other tools that don't need anything beyond what the image provides should also run.

The default image is built from `node:22-bookworm-slim` (Debian underneath) with `git`, `curl`, `ca-certificates`, `bubblewrap`, `iptables`, `jq`, `gh`, `dnsutils`, `unzip`, `less`, and `procps` installed via `apt`, plus Claude Code and Codex installed via their own official native installer scripts (not `npm install -g` — see the Dockerfile for why). It is not published to a registry — the Dockerfile is embedded in the `stashbase` binary itself, so a plain installed copy of the CLI can build it locally without needing this source repository. The first `agent run` that selects the Docker backend detects the image is missing and offers to build it (interactively; `--silent` runs fail closed instead of prompting). The build streams Docker's own progress live rather than running silently.

### Custom images

A profile can run a different image instead of the built-in default, either your own pre-built image or a custom Dockerfile — for example to add a language toolchain, a package manager, or other tools your agent needs:

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

A minimal base image needs `ca-certificates` installed for TLS-intercepted HTTPS requests to work — without it, the image's HTTP client can't validate the proxy's injected CA and HTTPS calls fail with a certificate error even though the request itself was allowed by policy. Verified with a plain `alpine:latest` image: unencrypted HTTP calls to allowed and denied hosts behave correctly out of the box, but HTTPS needs `apk add ca-certificates` (or the base image's equivalent) first.

`--docker-image <ref>` and `--docker-dockerfile <path>` override `sandbox.image`/`sandbox.dockerfile` for a single invocation, the same way `--docker-sandbox` overrides `sandbox.backend` — useful for trying a different image without editing the profile file. Either flag implies the Docker backend for that run even if the profile declares `backend = "native"` (or doesn't set `[sandbox]` at all), and the two are mutually exclusive with each other:

```bash
stashbase agent run --profile coding --docker-image node:22-alpine -- claude
```

### Git identity

Your global `git config user.name` and `user.email` (if configured on the host) are forwarded into the container as `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, and `GIT_COMMITTER_EMAIL`. This is the one piece of host configuration deliberately forwarded despite the filesystem allow-list, since it's authorship metadata, not a credential — without it, `git commit` inside the sandbox fails with no identity configured. It does not grant push access: `git push` (or any other authenticated git operation) still needs a real credential, wired through `[secrets]` like `GITHUB_TOKEN` in the example above, or run from outside the sandbox. Raw SSH keys are never forwarded. A profile that explicitly sets one of these four env vars itself takes precedence over the forwarded host value.

### Login persistence

Agent login/config state (e.g. Claude Code's `~/.claude`) is kept in a Docker-managed named volume, not a bind mount of your real home directory, so it survives across `agent run` invocations without exposing anything else on the host. This volume is shared across every profile and project using the Docker backend on this machine — logging in once covers all of them.

### Codex and subscription login

Codex's normal OAuth login flow opens a browser that redirects to a local HTTP callback server. That callback listens inside the container's own network namespace, which the host browser cannot reach — the container's `localhost` is not your machine's `localhost`. Use Codex's device-code flow instead, which doesn't depend on a local callback at all:

```bash
stashbase agent run --profile coding -- codex login --device-auth
```

### Docker backend limitations

- The container image is fixed and not user-configurable in this release; a workflow needing a tool outside the image's contents (a compiler, SSH, etc.) isn't supported yet.
- Two containers run per invocation (the network namespace holder plus the agent container itself), not one — slightly more setup overhead per run than a single-container approach, in exchange for the firewall being enforced by capability separation rather than a privilege drop inside the agent container.
- Teardown (stopping both containers, removing the per-run network) runs on normal exit, including Ctrl+C. A crash or forceful kill (`SIGKILL`) of the `stashbase` process itself can leave them behind rather than cleaned up.

This backend is early access, opt-in only, and does not change the default behavior of existing profiles.

## Network Access and HTTP Rules

By default, the proxy denies all connections. Allow specific destinations:

```toml
egress_hosts = ["api.github.com", "registry.npmjs.org"]
```

Use `"*"` for unrestricted internet (but this does not bypass credential rules—each secret still honors its own policy).

Optional: block specific hosts even when egress is broad:

```toml
egress_hosts = ["*"]
deny_hosts = ["api.stashbase.dev"]
```

## HTTP Credential Rules

Define which hosts, methods, and paths may receive each credential:

```toml
[secrets.GITHUB_TOKEN]
env = "GH_TOKEN"

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*/*", "/repos/*/*/issues*"]

[[secrets.GITHUB_TOKEN.rules]]
effect = "deny"
hosts = ["api.github.com"]
methods = ["DELETE"]
paths = ["*"]
```

- Rules are unordered; a matching `deny` always blocks the credential.
- Paths use `*` as a wildcard; query strings are ignored.
- At least one matching `allow` rule is required for injection.
- Methods are normalized to uppercase (`GET`, `POST`, etc.).

See [credential-policies.md](./agent-profiles/credential-policies.md) for detailed rule syntax.

## MCP Tool Restrictions

HTTP-based MCP servers can be configured with tool-level access control:

```toml
[secrets.LINEAR_API_KEY]
env = "LINEAR_API_KEY"

[[secrets.LINEAR_API_KEY.rules]]
effect = "allow"
hosts = ["mcp.linear.app"]
methods = ["GET", "POST"]
paths = ["/mcp"]

[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
binding = "LINEAR_API_KEY"
allow_tools = ["search_issues", "get_issue"]
```

The HTTP rule controls credential injection at the endpoint. The MCP rule controls which tools the agent may call. Omit `allow_tools` or leave it empty to deny all tools by default. Use `allow_tools = ["*"]` to allow all tools (a matching `deny_tools` still takes precedence).

## Dependency Hooks

If your profile enables authenticated hooks (e.g., dependency checking), grant capability explicitly:

```toml
allow_hooks = ["dependency_check"]
```

The child receives only a scoped local broker token. Hooks are disabled by default.

## Audit Logs and Session Revocation

`agent run` writes a local JSONL audit log of proxy events—host, method, action, status code, and byte counts. No secret values, placeholders, or request bodies are logged. Startup prints the audit session ID and log path.

View audit events:

```bash
stashbase agent logs list
stashbase agent logs --session <id> --action injected
```

List and revoke active sessions:

```bash
stashbase agent sessions list
stashbase agent sessions revoke <session-id>
```

## Personal Credentials

Personal credentials are account-specific secrets available only in remote sessions. They are never fetched, printed, or stored locally.

```toml
[personal_credentials.MY_TOKEN]
env = "MY_TOKEN"

[[personal_credentials.MY_TOKEN.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET", "POST"]
paths = ["/v1/*"]
```

See [personal-credentials.md](./agent-profiles/personal-credentials.md) for more details.

## Integration Examples

Common patterns for GitHub, Codex, Claude Code, and generic HTTP APIs:

See [integrations.md](./agent-profiles/integrations.md) for practical examples.

## Validate and Inspect Profiles

```bash
stashbase agent validate --profile my-profile
stashbase agent profiles show my-profile
stashbase agent explain --profile my-profile \
  --host api.github.com --method GET --path /user
```

## Limitations

The proxy is HTTP/HTTPS only and designed for standard developer tools. It does not support:
- SSH, Git-over-SSH, raw TCP/UDP, databases, local sockets
- Tools that bypass `HTTP_PROXY` environment variables
- Request-body or query-parameter injection (credentials are header-only)
- Process-level isolation (same-user processes can still access broader system credentials)

For stronger filesystem and network isolation than the native backend provides, see [Sandbox Backend](#sandbox-backend) above (experimental).

## Full Reference

This is a lightweight local guide. For comprehensive documentation on advanced features, policy regression tests, compatibility matrices, and operational details, see https://docs.stashbase.dev/agents.
