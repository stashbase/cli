# Agent broker profile cookbook

These are copy-paste starting points for `.stashbase.toml` in a trusted working
directory. Run one with:

```bash
stashbase agent run --profile <name> -- <command>
```

The default profile source is `auto`: Stashbase uses `./.stashbase.toml` when
present and otherwise falls back to the user-level config. Use
`--profile-source directory` to require the current directory's profile.

By default, the broker exchanges placeholders in an exact
`Authorization: Bearer <placeholder>` request header. Set `header` to support
another HTTP header; the default value template is `{secret}` for a custom
header. A profile's `hosts` controls where that secret may be injected;
`egress_hosts` permits ordinary traffic without injecting a credential. Keep
the two lists separate.

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
within a profile. Never place secret values or API keys in an untrusted file.

## Full-stack coding-agent profile

One profile can grant several independent capabilities to one coding-agent
session and any HTTP(S)-aware tools it launches. Each secret has its own source
binding, destination allowlist, and header representation; `egress_hosts` is
for ordinary traffic that must never receive a credential.

```toml
# .stashbase.toml
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
| Request bodies, query parameters, cookies, or arbitrary CLI arguments | No | Injection is header-only. Do not put real credentials in another channel to work around this. |
| SSH, Git-over-SSH, databases, raw TCP/UDP, local sockets | No | These protocols do not use the HTTP(S) broker. |
| Proxy-bypassing tools | No containment by default | They can connect directly unless they honor the proxy settings. macOS `--sandbox` limits direct network access to the broker loopback port; Linux and Windows sandbox support is not implemented. |
| HTTP/2 proxy clients, WebSockets, SSE, or large streaming uploads/downloads | Not a supported target | This proof-of-concept proxy accepts HTTP/1 proxy traffic and buffers request bodies before forwarding. |

The broker is not a general-purpose proxy, policy engine, or network firewall.
It is a short-lived credential-injection boundary for supported HTTP(S) tools.

## Current boundary

This remains an HTTP(S) broker. It cannot inject credentials into local-only
commands, SSH, databases, raw TCP, or tools that bypass proxy environment
variables. Do not work around that boundary by exposing a real secret to the
child process.

It reduces exposure during normal local agent and developer-tool workflows; it
is not a defense against a malicious or compromised same-user process. A
same-user process can potentially inspect local files or process memory, alter
the environment, or invoke ordinary Stashbase commands. Without macOS
`--sandbox`, proxy-bypassing tools can make direct network connections. The
macOS sandbox limits that network bypass but is not filesystem or process-memory
isolation. Directory profiles are trusted policy: review a repository's
`.stashbase.toml` before granting it secrets.

## Audit logs

`agent run` writes a local, metadata-only JSONL audit log by default. Startup
prints an audit session ID and the local log path. Events include the profile,
broker action, destination host, secret name, response status, and duration.
They never include secret values, placeholders, headers, bodies, URLs, or
command arguments.

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
