# Docker Sandbox Backend — Design

## Context

`stashbase agent run` today enforces filesystem/network containment via
platform-native mechanisms: Seatbelt (`sandbox-exec`) on macOS, and
`systemd-run --user` with bubblewrap fallback on Linux
(`src/handlers/run/subprocess.rs`). Unsupported platforms fail closed —
the run is refused rather than proceeding unsandboxed. This is documented
in `docs/agent-profiles.md` as "early access — local exposure reduction,
not hostile-agent isolation," and that doc explicitly notes: "For complete
network isolation, use a container or VM."

This design adds Docker as an additional, opt-in sandbox backend for
stronger isolation than the native mechanisms provide, without changing
default behavior for existing users.

## Goals

- Give users a way to run agent commands with stronger isolation
  (separate network namespace, container filesystem) than Seatbelt/
  bubblewrap offer today.
- Preserve the existing credential-proxy model: no raw secrets are ever
  passed into the sandboxed environment, only placeholders + proxy access.
- Preserve the existing fail-closed philosophy: if Docker isn't available
  or setup fails, the run is refused, never silently unsandboxed.

## Non-goals (v1)

- Not a default or fallback backend — native mechanisms remain the
  default on macOS/Linux.
- Not a path to Windows support in this iteration (a natural side effect
  of the design, since `docker run` isn't OS-specific, but out of scope
  to commit to here).
- No user-configurable container image. The image is a single built-in
  default maintained by this project; not exposed in the profile schema.
- No general-purpose Docker network allowlisting — all real egress
  continues to go through the existing HTTP credential proxy.

## Profile schema

New optional table on `AgentProfile` (`src/models/agent.rs`), parallel to
the existing `filesystem` table:

```toml
[sandbox]
backend = "docker"   # default: "native" (today's Seatbelt/bubblewrap behavior)
```

```rust
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AgentSandboxProfile {
    #[serde(default)]
    pub backend: SandboxBackend,
}

#[derive(Debug, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SandboxBackend {
    #[default]
    Native,
    Docker,
}
```

`AgentProfile` gains `pub sandbox: AgentSandboxProfile` (`#[serde(default)]`).

This is the first explicit backend-selection enum in the sandboxing code;
today's `subprocess.rs` is entirely `#[cfg(target_os)]` branches with no
shared abstraction. `SandboxBackend` gives future backends (if any) a
clean seam instead of a fourth cfg-branch tangle, but this design does not
refactor the existing Seatbelt/bubblewrap code paths to use it — it only
adds the new Docker path behind the enum.

## Data flow

`backend` is threaded through `entry.rs` / `proxy.rs` exactly the way
`denied_read_paths` / `denied_write_paths` already are today (same
pattern: read from `profile.sandbox.backend` at
`src/handlers/entry/root.rs`, carried through the run-proxy-policy struct
in `src/handlers/run/proxy.rs`, passed into the command-building call in
`src/handlers/run/entry.rs`).

At the call site currently occupied by
`sandbox_command_with_filesystem_policy` (`subprocess.rs:94`), the backend
enum picks between:
- `Native` → existing behavior, unchanged.
- `Docker` → new function in a new sibling module,
  `src/handlers/run/docker_sandbox.rs` (kept separate from
  `subprocess.rs`, which is already large and holds Seatbelt/bubblewrap
  arg-building — Docker's concerns, image resolution, mount args, network
  setup, are distinct enough to warrant their own module).

## Networking

The container must reach the existing credential-injecting HTTP proxy,
which binds to loopback on the host today. A container in its own network
namespace cannot see the host's `127.0.0.1`, and binding the proxy to
`0.0.0.0` would regress isolation (reachable from the whole LAN, not just
the sandboxed process).

Design: **per-run ephemeral bridge network.**

- For each `agent run` invocation using the Docker backend, create a
  fresh Docker network (`docker network create` with a random/UUID name).
- The proxy binds to that network's gateway address, not `0.0.0.0` and
  not loopback-only (loopback wouldn't be reachable from the container).
- The container is attached only to this network. `HTTPS_PROXY` /
  `HTTP_PROXY` env vars inside the container point at the gateway address.
- No other inbound/outbound Docker network rules are configured — the
  per-run network's isolation means nothing else needs an explicit deny;
  only the one container on that network can reach the proxy, and the
  network is torn down (`docker network rm`) when the run exits.
- Native-backend runs are unaffected: the proxy continues to bind
  loopback-only in that path.

Rejected alternatives:
- Shared default bridge + `0.0.0.0` bind: reachable from the LAN, a real
  isolation regression.
- Unix domain socket bind-mounted into the container: most isolated in
  theory (no TCP port at all), but inconsistent support for unix-socket
  proxies across `HTTP_PROXY`/`HTTPS_PROXY`-consuming tools risks breaking
  the exact tools being sandboxed. Revisit later if needed.
- `--network host`: shares the host's network namespace entirely, the
  opposite of the isolation goal. Rejected outright.

## Filesystem

Docker containers see nothing from the host by default. Instead of
replicating `deny_read`/`deny_write` as deny-lists (the native backends'
approach), the Docker backend takes an allow-list approach:

- One bind mount: the current working directory, read-write, at the same
  path inside the container.
- Nothing else from the host filesystem is visible. This exceeds today's
  guarantee for paths outside the cwd (e.g. `~/.ssh`, `~/.aws` are
  invisible, not merely denied).
- **Nested deny paths**: if a `deny_read` or `deny_write` entry falls
  inside the cwd (e.g. denying `.git` while the whole project is mounted),
  shadow-mount over that subpath inside the container: an empty `tmpfs`
  for `deny_read`, a read-only bind mount of the same path for
  `deny_write`. This preserves the existing guarantee instead of silently
  dropping it for the Docker backend.

## Credentials / env vars

Matches the existing proxy model: no raw secrets are ever placed in the
container's environment. Only the proxy placeholder env vars and
`HTTPS_PROXY`/`HTTP_PROXY` (pointing at the per-run network gateway) are
passed through, same as the native backends today.

## Container image

Single built-in default image, not configurable in v1 (explicitly
descoped per user decision — no `image` field on `AgentSandboxProfile`).
The default should be a minimal, maintained base sufficient for common
agent/CLI workloads. Exact image choice and maintenance process
(versioning, rebuild cadence, contents) is an implementation detail for
the plan, not fixed by this design.

## Error handling / fail-closed behavior

Consistent with the existing philosophy (unsupported platforms fail
closed today):
- Docker not installed, or daemon not reachable → run refused with a
  clear error, never falls back to unsandboxed execution.
- Per-run network creation, proxy bind, or container start failure → run
  refused, network cleaned up if partially created.
- Container exit code / stdout / stderr are surfaced to the caller the
  same way native sandboxed runs are today.

## Testing

- Unit tests for `docker_sandbox.rs`'s command/argument construction
  (network create args, mount args including shadow-mounts for nested
  deny paths, env var filtering), mirroring the existing arg-construction
  tests for `bubblewrap_command` in `subprocess.rs`.
- These tests do not require Docker installed (they assert on constructed
  argv, same pattern as existing bubblewrap tests).
- A smaller set of integration tests gated behind Docker availability
  (skipped in environments without Docker, similar to how Linux-only
  sandbox tests are already gated) to verify actual container isolation:
  proxy reachable from inside the container, denied paths inaccessible,
  network unreachable to anything outside the per-run network.

## Open questions for the implementation plan

- Exact default image contents/tag and how it's published/versioned.
- Whether `docker network create`/`rm` overhead per run is acceptable
  latency-wise, or whether a longer-lived pool of pre-created networks is
  worth it (v1 should just measure; optimize only if it's a real problem).
