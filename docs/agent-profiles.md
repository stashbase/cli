# Agent broker profile cookbook

> **Early access — local exposure reduction, not hostile-agent isolation.**
> Agent profiles keep their granted secrets out of a child environment and
> inject them only through the supported HTTP(S) broker. They do not prevent a
> malicious process running as the same user from accessing broader Stashbase
> credentials or bypassing this workflow.

These are copy-paste starting points for `stashbase-agent.toml` in a trusted working
directory. Run one with:

```bash
stashbase agent run --profile <name> -- <command>
```

## Remote broker sessions

For a project/environment-backed profile whose secrets are stored in Stashbase,
add `--remote` to resolve and retain credentials only in the control plane:

```bash
stashbase agent run --remote --profile coding -- codex
```

The CLI authenticates normally, creates one 10-minute scoped session, and
passes only `${STASHBASE_SECRET_NAME}` placeholders to the child. The opaque
token is memory-only and revoked when the child exits. **Remote Broker Beta is
not a generic proxy:** it is only for integrations that explicitly use the
Stashbase custom request transport. HTTP/1 WebSocket upgrades are relayed for
agent streaming connections; HTTP/2, generic MCP proxying, browsers, and
ordinary SDK proxy configuration are outside the Beta transport scope.

The default profile source is `auto`: Stashbase uses `./stashbase-agent.toml` when
present and otherwise falls back to the user-level config. Use
`--profile-source directory` to require the current directory's profile.

Validate a profile before granting it secrets—locally or in CI:

```bash
stashbase agent validate --profile coding
stashbase agent validate --profile coding --profile-source directory
stashbase agent validate --profile coding --json
```

Validation does not fetch or read secret values and does not start a broker. It
checks the selected source, local-file availability, duplicate `from` bindings,
child environment-variable names, host rules, custom header names, and value
templates. `egress_hosts = ["*"]` is valid but reported as a warning.

By default, the broker exchanges placeholders in an exact
`Authorization: Bearer <placeholder>` request header. Set `header` to support
another HTTP header; the default value template is `{secret}` for a custom
header. A profile's `hosts` controls where that secret may be injected;
`egress_hosts` permits ordinary traffic without injecting a credential. Keep
the two lists separate.

`deny_hosts` is an optional final override. It uses the same exact-host and
`*.subdomain.example` syntax as `egress_hosts`; a matching deny blocks the
destination even when wildcard egress or a secret's `hosts` list would allow
it. This practical local-agent profile allows Codex and MCP tools to use the
internet while preventing a nested Stashbase CLI from calling the API:

```toml
egress_hosts = ["*"]
deny_hosts = ["api.stashbase.dev"]
```

For a custom deployment, deny the hostname from `STASHBASE_API_URL` instead.

## Egress-only profiles

An agent profile may omit `file`, `project`, `environment`, and `secrets`
entirely. It starts the broker solely to enforce egress policy and grants no
Stashbase-managed credentials—useful for Codex with an existing local login or
for MCP-only workflows:

```toml
[agent_profiles.codex]
egress_hosts = ["chatgpt.com", "mcp.context7.com", "mcp.linear.app"]
deny_hosts = ["api.stashbase.dev"]
```

The CLI prints an egress-only warning at startup. To prevent accidental secret
loading, this mode rejects a configured `file`, `project`, or `environment`.

## Secret sources, bindings, and local overrides

Without `from`, each secret-table key is also its remote source name. This
profile fetches only `GH_TOKEN` and `OPENAI_API_KEY` with the Stashbase API's
`only` query; it never fetches the entire environment.

```toml
[agent_profiles.coding]
project = "platform"
environment = "development"

[agent_profiles.coding.secrets.GH_TOKEN]
hosts = ["api.github.com"]

[agent_profiles.coding.secrets.OPENAI_API_KEY]
hosts = ["api.openai.com"]
```

Set `from` when the remote canonical name differs from the tool-facing name.
The API fetches the source name, while the child receives only the profile key
as a placeholder:

```toml
[agent_profiles.copilot.secrets.GH_TOKEN]
from = "GITHUB_TOKEN"
hosts = ["api.github.com"]
```

You can combine a remote source with a local override file. The file is read
first; for every configured source it supplies, no remote request is made. Any
source absent from the file is fetched from the configured project/environment.
The local value wins when both sources define it.

```toml
[agent_profiles.coding]
project = "platform"
environment = "development"
file = ".env.local"

[agent_profiles.coding.secrets.GH_TOKEN]
from = "GITHUB_TOKEN"
hosts = ["api.github.com"]

[agent_profiles.coding.secrets.OPENAI_API_KEY]
hosts = ["api.openai.com"]
```

With `GITHUB_TOKEN` in `.env.local`, Stashbase requests only
`OPENAI_API_KEY`. The child receives placeholders named `GH_TOKEN` and
`OPENAI_API_KEY`, never their real values. Each `from` source may be bound once
within a profile. File-only profiles do not require a Stashbase API key; a key
is required only when one or more remote sources are needed. Never place secret
values or API keys in an untrusted file.

## Full-stack coding-agent profile

One profile can grant several independent capabilities to one coding-agent
session and any HTTP(S)-aware tools it launches. Each secret has its own source
binding, destination allowlist, and header representation; `egress_hosts` is
for ordinary traffic that must never receive a credential.

```toml
# stashbase-agent.toml
[agent_profiles.full-stack]
project = "platform"
environment = "development"
file = ".env.local" # Optional local overrides for the source names below.
egress_hosts = [
  "registry.npmjs.org",
  "pypi.org",
  "files.pythonhosted.org",
  "docs.rs",
]

# GitHub CLI / Copilot: remote GITHUB_TOKEN becomes GH_TOKEN for the child.
[agent_profiles.full-stack.secrets.GH_TOKEN]
from = "GITHUB_TOKEN"
hosts = [
  "api.github.com",
  "github.com",
  "copilot-proxy.githubusercontent.com",
  "*.githubcopilot.com",
]

# OpenAI-compatible clients use the default Authorization: Bearer header.
[agent_profiles.full-stack.secrets.OPENAI_API_KEY]
hosts = ["api.openai.com"]

# Anthropic uses a dedicated API-key header.
[agent_profiles.full-stack.secrets.ANTHROPIC_API_KEY]
hosts = ["api.anthropic.com"]
header = "x-api-key"

# A third-party service can use any configured header and source binding.
[agent_profiles.full-stack.secrets.PARTNER_API_KEY]
from = "PLATFORM_PARTNER_KEY"
hosts = ["api.partner.example"]
header = "x-api-key"
```

Run the session normally:

```bash
stashbase agent run --profile full-stack -- codex
```

If `.env.local` contains `GITHUB_TOKEN`, the remote request omits that source;
the API receives an `only` list for the remaining sources. The child receives
only `GH_TOKEN`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and
`PARTNER_API_KEY` placeholders. It never receives a real secret value.

## GitHub Copilot CLI

GitHub documents Copilot's GitHub, Copilot service, and telemetry endpoints in
its [Copilot allowlist reference](https://docs.github.com/en/copilot/reference/copilot-allowlist-reference).
This profile lets `GH_TOKEN` authenticate only to the GitHub/Copilot service
endpoints, while telemetry has egress-only access.

```toml
[agent_profiles.copilot]
file = "./.env.agent"
egress_hosts = [
  "collector.github.com",
  "copilot-telemetry.githubusercontent.com",
  "default.exp-tas.com",
]

[agent_profiles.copilot.secrets.GH_TOKEN]
hosts = [
  "api.github.com",
  "github.com",
  "copilot-proxy.githubusercontent.com",
  "origin-tracker.githubusercontent.com",
  "*.githubcopilot.com",
]
```

Run:

```bash
stashbase agent run --profile copilot --profile-source directory -- copilot
```

Copilot recognizes `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, and `GITHUB_TOKEN` in
that precedence order. See the [Copilot CLI programmatic reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference).

## OpenAI API clients

Use this for an API-key-based coding tool or script that sends
`OPENAI_API_KEY` as a Bearer token to the OpenAI API. OpenAI documents API-key
authentication in its [API reference](https://developers.openai.com/api/reference/overview#authentication).

```toml
[agent_profiles.openai-api]
file = "./.env.agent"

[agent_profiles.openai-api.secrets.OPENAI_API_KEY]
hosts = ["api.openai.com"]
```

## Generic OpenAI-compatible endpoint

For a self-hosted or third-party OpenAI-compatible API that uses a Bearer API
key, replace the host and variable name:

```toml
[agent_profiles.compatible-api]
file = "./.env.agent"

[agent_profiles.compatible-api.secrets.PROVIDER_API_KEY]
hosts = ["llm.example.com"]
```

## Anthropic / Claude Code

Anthropic's API uses `x-api-key`, so configure that header explicitly. The
[Anthropic API overview](https://platform.claude.com/docs/en/api/overview)
documents this authentication format.

```toml
[agent_profiles.claude]
file = "./.env.agent"

[agent_profiles.claude.secrets.ANTHROPIC_API_KEY]
hosts = ["api.anthropic.com"]
header = "x-api-key"
```

## Gemini API clients

Gemini API requests use `x-goog-api-key`, which is supported with the same
custom-header mechanism. See the [Gemini API reference](https://ai.google.dev/api).

```toml
[agent_profiles.gemini]
file = "./.env.agent"

[agent_profiles.gemini.secrets.GEMINI_API_KEY]
hosts = ["generativelanguage.googleapis.com"]
header = "x-goog-api-key"
```

## General egress

Agents often need package registries, source control, or documentation access.
List those destinations in `egress_hosts`. If unrestricted HTTP(S) egress is
intentional, use the explicit wildcard below. It does **not** allow a secret to
be injected anywhere beyond its own `hosts` list.

```toml
egress_hosts = ["*"]
```

### Stashbase API access is a profile decision

`egress_hosts` is host-based. If it includes your Stashbase API host—or uses
the `"*"` wildcard without a matching `deny_hosts` entry—the child may be able to use ordinary Stashbase CLI commands
such as project/environment discovery or `stashbase secrets list` through the
broker. With a normal personal or service API key available in the
operating-system credential store, those commands may authenticate as the
developer and retrieve secrets the developer is authorized to access.

For a tight coding profile, allow only the tool destinations the agent needs:

```toml
egress_hosts = ["api.github.com", "registry.npmjs.org"]
```

For compatible local development, allow ordinary egress but deny the API as a
final override:

```toml
egress_hosts = ["*"]
deny_hosts = ["api.stashbase.dev"]
```

Then a child request to an unlisted Stashbase API host is denied and recorded
as `host_denied` in the broker audit log. Some HTTPS clients surface that
CONNECT-level denial as a generic connection error. Use `--sandbox` on supported platforms as well when direct network
bypass must be blocked. Allowing broad egress is an explicit developer trust
decision; the CLI does not implement fragile path-by-path rules for Stashbase
endpoints. Future scoped agent-session tokens will let the API enforce finer
permissions server-side.

## Compatibility and proxy limits

The broker is intentionally focused on common developer-tool HTTP(S) traffic.
Use this matrix when deciding whether a workflow belongs in an agent profile.

| Workflow or protocol | Broker support | Notes |
| --- | --- | --- |
| `curl` and ordinary HTTP clients | Yes | The client must honor `HTTP_PROXY` / `HTTPS_PROXY` and place the placeholder in a configured header. |
| HTTPS APIs | Yes, with temporary CA trust | Most clients use the CA-file variables supplied by the CLI. Use `--trust-broker-ca` only when a client requires operating-system trust-store integration. |
| Node.js / `fetch` | Usually | The CLI enables `NODE_USE_ENV_PROXY`; use a Node runtime that supports environment proxy settings. |
| `gh` and GitHub Copilot CLI | Usually | Configure every required GitHub/Copilot host. Some builds need `--trust-broker-ca`. |
| Agent-spawned HTTP tools | Yes | They inherit the placeholders and proxy variables from the agent process. The same broker handles every descendant; no nested broker is needed. |
| Custom API-key headers | Yes | Configure `header` and, when needed, `value_template`. |
| Streaming uploads, downloads, and SSE | Yes over HTTP/1 | Bodies are forwarded incrementally and unchanged; credential replacement remains header-only. |
| Request bodies, query parameters, cookies, or arbitrary CLI arguments | No | Injection is header-only. Do not put real credentials in another channel to work around this. |
| SSH, Git-over-SSH, databases, raw TCP/UDP, local sockets | No | These protocols do not use the HTTP(S) broker. |
| Proxy-bypassing tools | No containment by default | They can connect directly unless they honor the proxy settings. `--sandbox` limits direct network access to the broker loopback port on macOS and systemd-based Linux; Windows is not implemented. |
| WebSockets over HTTP/1 (`wss://`) | Yes | The broker tunnels the upgraded connection after applying host policy and header placeholder rewriting. This supports Codex streaming connections. |
| HTTP/2 proxy clients | Not a supported target | This proof-of-concept proxy accepts HTTP/1 proxy traffic only. |

The broker is not a general-purpose proxy, policy engine, or network firewall.
It is a short-lived credential-injection boundary for supported HTTP(S) tools.

Before adding a new tool to a workflow, run the local compatibility report:

```bash
stashbase agent doctor curl
stashbase agent doctor gh
stashbase agent doctor copilot
stashbase agent doctor codex
```

It never loads a profile or secret. It verifies that the executable is present,
starts a temporary no-secret broker, confirms the proxy and temporary CA
environment it would pass to a child, and reports known compatibility guidance.
It cannot prove that every release or plugin inside a third-party tool will
honor proxy settings, so also perform an allowed-host end-to-end test.

## Current boundary

This remains an HTTP(S) broker. It cannot inject credentials into local-only
commands, SSH, databases, raw TCP, or tools that bypass proxy environment
variables. Do not work around that boundary by exposing a real secret to the
child process.

In broker mode, Stashbase clears inherited `NO_PROXY` / `no_proxy`,
`ALL_PROXY` / `all_proxy`, and npm proxy override variables before starting the
child, then supplies its own `HTTP_PROXY` and `HTTPS_PROXY`. This prevents the
most common accidental bypasses. A tool can still intentionally use its own
direct connection or proxy configuration; use `--sandbox` when direct network
egress must be blocked.

It reduces exposure during normal local agent and developer-tool workflows; it
is not a defense against a malicious or compromised same-user process. A
same-user process can potentially inspect local files or process memory, alter
the environment, or invoke ordinary Stashbase commands. Without `--sandbox`,
proxy-bypassing tools can make direct network connections. The sandbox limits
that network bypass but is not filesystem or process-memory isolation. `agent run` removes an inherited `STASHBASE_API_KEY` environment
variable as defense in depth, but this does not protect credentials stored in
CLI configuration or the operating-system credential store. Directory profiles
are trusted policy: review a repository's `stashbase-agent.toml` before granting it
secrets.

## Audit logs

`agent run` writes a local, metadata-only JSONL audit log by default. Startup
prints an audit session ID and the local log path. Events include the profile,
broker action, destination host, secret name, response status, and duration.
They never include secret values, placeholders, headers, bodies, URLs, or
command arguments.

Common diagnostic actions are `host_denied`, `unknown_placeholder`,
`tls_trust_failed`, `upstream_timeout`, `upstream_connection_failed`, and
`upstream_response_failed`. For example:

```bash
stashbase agent logs --action host_denied
stashbase agent logs --action tls_trust_failed --since 1h
```

`unknown_placeholder` means a placeholder from another or stale session was
blocked before it could be forwarded. `tls_trust_failed` means the HTTPS
handshake ended while the broker's temporary certificate was being presented;
the protocol cannot reveal the exact client-side trust error. A direct proxy
bypass cannot be logged because no request reaches the broker—use the macOS
`--sandbox` option when that containment matters.

```text
Audit session: 5fd2...
Audit log: .../stashbase/audit/agent-5fd2....jsonl
```

Inspect recent decisions without reading JSONL files directly:

```bash
stashbase agent logs
stashbase agent logs --session 5fd2...
stashbase agent logs --profile coding --action injected --host api.github.com
stashbase agent logs --since 24h --limit 100
stashbase agent logs --follow
```

`--json` returns a JSON array for a one-time view; with `--follow`, it emits
one JSON event per line. Logs older than 30 days are removed automatically and
the local store is capped at 1,000 session files. Disable persistence for one
run with `--audit-log false`:

```bash
stashbase agent run --profile coding --audit-log false -- codex
```

## Troubleshooting

When a tool reports a proxy 403, run once with `RUST_LOG=debug`. The broker
prints only the denied destination host, never the secret or request headers.
Add that host either to the relevant secret's `hosts` (if it must receive the
credential) or to `egress_hosts` (if it must not).
