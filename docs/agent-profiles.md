# Agent Proxy profile cookbook

> **Early access — local exposure reduction, not hostile-agent isolation.**
> Agent profiles keep their granted secrets out of a child environment and
> inject them only through the supported HTTP(S) proxy. They do not prevent a
> malicious process running as the same user from accessing broader Stashbase
> credentials or bypassing this workflow.

For a repository with several agents, put each direct profile in a trusted
working directory at `.stashbase/agents/<name>.toml`. Run one with:

```bash
stashbase agent run --profile <name> -- <command>
```

## Remote Agent Proxy sessions

For a project/environment-backed profile whose secrets are stored in Stashbase,
add `--remote` to resolve and retain credentials only in the control plane:

```bash
stashbase agent run --remote --profile coding -- codex
```

The CLI authenticates normally, creates one short-lived scoped session, and
passes only `${STASHBASE_SECRET_NAME}` placeholders to the child. The opaque
session token is memory-only and revoked when the child exits.

The child uses a temporary localhost proxy through its normal `HTTP_PROXY` and
`HTTPS_PROXY` settings. That relay attaches the session token to the remote
Agent Proxy and keeps both the token and resolved secret values out of the
child environment. Forward-proxy TLS-intercept sessions also provision the
remote public CA for the child, enabling supported existing coding agents such
as Codex, Copilot, and Claude Code to use their ordinary HTTP(S) transports.

Remote Agent Proxy is not a general network sandbox: SSH, databases, raw TCP,
browsers, and a tool that deliberately bypasses proxy settings are outside its
scope. HTTP/1 WebSocket upgrades used by supported coding agents are relayed;
HTTP/2 proxying and arbitrary third-party proxy integrations remain unsupported.

The default profile source is `auto`: Stashbase uses
`./.stashbase/agents/<profile>.toml` when present, then the legacy
`./stashbase-agent.toml` format, and otherwise falls back to user-level config.
Use `--profile-source directory` to require a repository-local profile. A
profile defined in both layouts is rejected instead of silently overriding one.
At startup, Stashbase warns when the selected repository-local profile is
tracked by Git and has staged/unstaged changes. Untracked files are allowed so
personal policy files do not create noise. This is a review signal only; it
does not block a run. `--silent` suppresses the warning.

Each file in `.stashbase/agents` contains the profile directly—there is no
`[agent_profiles.<name>]` wrapper:

```toml
# .stashbase/agents/codex.toml
project = "local-agents"
environment = "local-creds"
egress_hosts = ["api.github.com"]

[secrets.GITHUB_TOKEN]
from = "GITHUB_TOKEN"
header = "Authorization"
value_template = "Bearer {secret}"
```

Validate a profile before granting it secrets—locally or in CI:

```bash
stashbase agent validate --profile coding
stashbase agent validate --profile coding --profile-source directory
stashbase agent validate --profile coding --json
stashbase agent validate --remote --profile coding
```

Discover repository-local and user-level profiles without opening their files:

```bash
stashbase agent profiles list
stashbase agent profiles list --profile-source directory
stashbase agent profiles show coding
stashbase --json agent profiles show coding
```

`list` shows each profile's selected source and a small capability summary.
`show` prints the configured policy only; it never loads or displays secret
values.

For CI or explicit automation, bypass global/current-directory discovery with a
direct profile file. It uses the direct-file format and is mutually exclusive
with `--profile-source`:

```bash
stashbase agent validate --profile codex --policy-file ci/agents/codex.toml
stashbase agent run --profile codex --policy-file .stashbase/agents/codex.toml -- codex
stashbase agent explain --profile codex --policy-file ci/agents/codex.toml \
  --host api.github.com --method GET --path /user
```

Explain a prospective request without loading a secret, starting a proxy, or
opening a network connection:

```bash
stashbase agent explain --profile coding \
  --host api.github.com --method GET --path /user
```

The explanation reports the global connection decision and whether each
configured credential would be eligible for injection. It never prints a
secret value or placeholder.

Validation does not fetch or read secret values and does not start a proxy. It
checks the selected source, local-file availability, duplicate `from` bindings,
child environment-variable names, host rules, custom header names, and value
templates. It also warns about duplicate HTTP rules, all-path `"*"` rules, and
allows that a deny rule fully shadows. These warnings do not reject an
intentional policy. `egress_hosts = ["*"]` is also valid but reported as a
warning.

Add `--remote` before a remote run to also verify that the profile is compatible
with a project/environment-backed remote session and inspect cached public
Agent Proxy CAs at `~/.stashbase/remote-proxy/remote-proxy-<key_id>.pem`. On first use,
the CLI provisions that public CA from the authenticated session response,
verifies its SHA-256 digest, and caches it atomically. A missing cache is a
warning, not a validation failure. This preflight does not authenticate, fetch
secrets, or create a remote session.

By default, the proxy exchanges placeholders in an exact
`Authorization: Bearer <placeholder>` request header. Set `header` to support
another HTTP header; the default value template is `{secret}` for a custom
header. The three destination controls are intentionally separate:

- When configured, `egress_hosts` controls where the agent may connect,
  including requests that carry no Stashbase credential.
- `secrets.<name>.hosts` is the legacy credential host allowlist. It remains in
  effect when that secret has no `rules`.
- `secrets.<name>.rules` controls which HTTP methods and URL paths may receive
  that particular credential. Rules do not widen ordinary egress.

### Credential rule evaluation

Rules are an unordered set: their order in TOML does not change the result.
Multiple `allow` rules are additive—a request may match any one of them. A
matching `deny` always wins, even when the same request also matches an
`allow`. If a secret has any rules, no matching `allow` means that secret is
not injected. This is the complete decision order for a request carrying a
secret placeholder:

1. A matching global `deny_hosts` entry denies the request.
2. When `egress_hosts` is configured, the destination must match it.
3. With no secret `rules`, the legacy `secrets.<name>.hosts` list must match.
4. With rules, any matching `deny` rejects the credential; otherwise at least
   one matching `allow` is required.
5. Only then does the proxy inject the credential.

Rules match host, HTTP method, and URL path. Methods are normalized to
uppercase. Query strings are ignored; paths are normalized before matching;
and `*` matches any sequence of path characters. Redirects are evaluated as
independent requests, so a credential is never forwarded to a redirected
destination without a fresh policy check.

For example, the two allows below form a union. A `GET /user` request, a
`PATCH` request, or any other unmatched route is denied for this credential:

```toml
egress_hosts = ["api.github.com"]

[secrets.github]
from = "GITHUB_TOKEN"
env = "GITHUB_TOKEN"
header = "Authorization"
value_template = "Bearer {secret}"

[[secrets.github.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*/*", "/repos/*/*/issues*"]

[[secrets.github.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["POST", "PATCH"]
paths = ["/repos/*/*/issues", "/repos/*/*/issues/*/comments"]

[[secrets.github.rules]]
effect = "deny"
hosts = ["api.github.com"]
methods = ["DELETE"]
paths = ["*"]
```

You can put a narrow safety exception inside a broad allow. The deny is still
effective regardless of rule order:

```toml
[[secrets.github.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*"]

[[secrets.github.rules]]
effect = "deny"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*/actions/secrets/*"]
```

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
entirely. It starts the proxy solely to enforce egress policy and grants no
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

### Explicit child variables and format-aware placeholders

`env` optionally names the environment variable given to the agent. It is
useful when the Stashbase source name or profile binding name differs from the
variable a tool expects. The child still receives an opaque placeholder, never
the secret value:

```toml
[agent_profiles.example.secrets.TOOL_TOKEN]
from = "PLATFORM_TOKEN"
env = "TOOL_API_KEY"
hosts = ["api.example.com"]
header = "x-api-key"
```

In remote mode, that produces `TOOL_API_KEY=${STASHBASE_TOOL_TOKEN}` in the
child environment. The remote proxy exchanges the placeholder only in the
configured `x-api-key` header for `api.example.com`.

Some clients validate an API-key shape before they send any request. For those
clients, `placeholder` can provide an opaque, syntactically compatible value:

```toml
placeholder = "provider-shaped-but-non-secret-placeholder"
```

This is a compatibility value, not a credential source. `from` still selects
the real secret from Stashbase, and the child never receives that real value.
The remote proxy exact-matches the configured safe placeholder before it
injects the mapped credential. The default
`${STASHBASE_BINDING_NAME}` remains available when `placeholder` is omitted.

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
binding, legacy host allowlist or HTTP action rules, and header representation;
`egress_hosts` controls connectivity and never causes credential injection.

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
  "copilot-broker.githubusercontent.com",
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
  "copilot-broker.githubusercontent.com",
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
documents this authentication format. `env` ensures Claude Code receives its
credential in the variable it recognizes, while the proxy retains control of
the real key.

```toml
[agent_profiles.claude]
project = "platform"
environment = "development"
egress_hosts = ["api.anthropic.com"]

[agent_profiles.claude.secrets.ANTHROPIC_API_KEY]
hosts = ["api.anthropic.com"]
header = "x-api-key"
env = "ANTHROPIC_API_KEY"
# Opaque format-compatible value; never a real Anthropic key.
placeholder = "sk-ant-api03-stashbase-placeholder-000000000000000000000000000000000000"
```

The child process sees only
`ANTHROPIC_API_KEY=${STASHBASE_ANTHROPIC_API_KEY}`. When Claude Code sends an
`x-api-key` request to `api.anthropic.com`, the proxy injects the real
Stashbase-managed key. A local API key or `ANTHROPIC_AUTH_TOKEN` should not be
relied upon for this profile-managed path.

The placeholder above lets Claude Code pass its local API-key format check; the
proxy still exchanges it only at `api.anthropic.com`. This preserves the same
proxy boundary; it merely lets the client begin the proxied request.

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
proxy. With a normal personal or service API key available in the
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
as `host_denied` in the proxy audit log. Some HTTPS clients surface that
CONNECT-level denial as a generic connection error. Use `--sandbox` on supported platforms as well when direct network
bypass must be blocked. Allowing broad egress is an explicit developer trust
decision; the CLI does not implement fragile path-by-path rules for Stashbase
endpoints. Future scoped agent-session tokens will let the API enforce finer
permissions server-side.

## Compatibility and proxy limits

The proxy is intentionally focused on common developer-tool HTTP(S) traffic.
Use this matrix when deciding whether a workflow belongs in an agent profile.

| Workflow or protocol | Proxy support | Notes |
| --- | --- | --- |
| `curl` and ordinary HTTP clients | Yes | The client must honor `HTTP_PROXY` / `HTTPS_PROXY` and place the placeholder in a configured header. |
| HTTPS APIs | Yes, with temporary CA trust | Most clients use the CA-file variables supplied by the CLI. Use `--trust-proxy-ca` only when a client requires operating-system trust-store integration. |
| Node.js / `fetch` | Usually | The CLI enables `NODE_USE_ENV_PROXY`; use a Node runtime that supports environment proxy settings. |
| `gh` and GitHub Copilot CLI | Usually | Configure every required GitHub/Copilot host. Some builds need `--trust-proxy-ca`. |
| Agent-spawned HTTP tools | Yes | They inherit the placeholders and proxy variables from the agent process. The same proxy handles every descendant; no nested proxy is needed. |
| Custom API-key headers | Yes | Configure `header` and, when needed, `value_template`. |
| Streaming uploads, downloads, and SSE | Yes over HTTP/1 | Bodies are forwarded incrementally and unchanged; credential replacement remains header-only. |
| Request bodies, query parameters, cookies, or arbitrary CLI arguments | No | Injection is header-only. Do not put real credentials in another channel to work around this. |
| SSH, Git-over-SSH, databases, raw TCP/UDP, local sockets | No | These protocols do not use the HTTP(S) proxy. |
| Proxy-bypassing tools | No containment by default | They can connect directly unless they honor the proxy settings. `--sandbox` limits direct network access to the proxy loopback port on macOS and systemd-based Linux; Windows is not implemented. |
| WebSockets over HTTP/1 (`wss://`) | Yes | The proxy tunnels the upgraded connection after applying host policy and header placeholder rewriting. This supports Codex streaming connections. |
| HTTP/2 proxy clients | Not a supported target | This proof-of-concept proxy accepts HTTP/1 proxy traffic only. |

The proxy is not a general-purpose proxy, policy engine, or network firewall.
It is a short-lived credential-injection boundary for supported HTTP(S) tools.

Before adding a new tool to a workflow, run the local compatibility report:

```bash
stashbase agent doctor curl
stashbase agent doctor gh
stashbase agent doctor copilot
stashbase agent doctor codex
stashbase agent doctor --remote codex
```

It never loads a profile or secret. It verifies that the executable is present,
starts a temporary no-secret proxy, confirms the proxy and temporary CA
environment it would pass to a child, and reports known compatibility guidance.
With `--remote`, it also verifies the remote Agent Proxy CA required for standard
forward-proxy TLS interception.
It cannot prove that every release or plugin inside a third-party tool will
honor proxy settings, so also perform an allowed-host end-to-end test.

## Current boundary

This remains an HTTP(S) proxy. It cannot inject credentials into local-only
commands, SSH, databases, raw TCP, or tools that bypass proxy environment
variables. Do not work around that boundary by exposing a real secret to the
child process.

In proxy mode, Stashbase clears inherited `NO_PROXY` / `no_proxy`,
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
are trusted policy: review a repository's `.stashbase/agents/*.toml` (or legacy
`stashbase-agent.toml`) before granting it secrets.

## Audit logs

`agent run` writes a local, metadata-only JSONL audit log by default. Startup
prints an audit session ID, policy fingerprint, and the local log path. The
fingerprint is a SHA-256 identifier of the normalized policy snapshot for that
run; it contains no secret values or placeholders. Events include the profile,
policy fingerprint, proxy action, destination host, secret name, response
status, and duration.
They never include secret values, placeholders, headers, bodies, URLs, or
command arguments.

The `session_started` event also records the selected profile source, that
file's RFC 3339 modification time, and a SHA-256 hash of its contents. This
ties a session to the reviewed profile revision without storing the policy
contents in every audit record.

Common diagnostic actions are `host_denied`, `unknown_placeholder`,
`tls_trust_failed`, `upstream_timeout`, `upstream_connection_failed`, and
`upstream_response_failed`. For example:

```bash
stashbase agent logs --action host_denied
stashbase agent logs --action tls_trust_failed --since 1h
```

`unknown_placeholder` means a placeholder from another or stale session was
blocked before it could be forwarded. `tls_trust_failed` means the HTTPS
handshake ended while the proxy's temporary certificate was being presented;
the protocol cannot reveal the exact client-side trust error. A direct proxy
bypass cannot be logged because no request reaches the proxy—use the macOS
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
stashbase agent logs --id evt_...
stashbase agent logs --since 24h --limit 100
stashbase agent logs --follow
```

To inspect the policy with defaults and matcher normalization resolved, without
loading a secret value:

```bash
stashbase agent profiles show codex --effective
stashbase agent profiles show codex --effective --json
```

The effective view fills omitted secret binding fields (`from`, `env`, local
proxy placeholder, header, and value template), lowercases hosts, uppercases
HTTP methods, and normalizes rule paths. It shows the local placeholder only,
never the resolved secret value.

`--json` returns a JSON array for a one-time view; with `--follow`, it emits
one JSON event per line. Logs older than 30 days are removed automatically and
the local store is capped at 1,000 session files. Disable persistence for one
run with `--audit-log false`:

```bash
stashbase agent run --profile coding --audit-log false -- codex
```

Every local audit event receives an opaque short UUID `id` with an `evt_`
prefix (for example, `evt_mhvXdrZT4jP5T8vBxuvm75`). A request event's ID is
also included as `error.id` in safe proxy errors, so a 403 or upstream failure
can be inspected with `agent logs --id`.
It is never forwarded to an upstream service or the remote Agent Proxy.

## Policy regression tests

Policy regression tests evaluate the same host, method, path, egress, and
deny precedence logic as the proxy. They never load a secret, start an agent,
or make a network request. Use them in CI to prevent an edit from widening or
breaking a reviewed capability:

```bash
stashbase agent policy test --profile codex
stashbase agent policy test --profile codex --test-file ci/agent-policy-tests.toml
```

Put cases directly in the profile with `[[policy_tests]]`:

```toml
[[policy_tests]]
name = "GitHub current user remains readable"
secret = "GITHUB_TOKEN"
method = "GET"
host = "api.github.com"
path = "/user"
expect = "allow"

[[policy_tests]]
name = "GitHub deletion remains denied"
secret = "GITHUB_TOKEN"
method = "DELETE"
host = "api.github.com"
path = "/repos/acme/app"
expect = "deny"
```

Or keep them in a separate TOML file, conventionally
`.stashbase/agent-policy-tests.toml`, using `[[tests]]` instead of
`[[policy_tests]]`. If embedded cases exist, the command uses them by default;
`--test-file` explicitly selects the separate file. A failed expectation exits
with status 1.

## Troubleshooting

When a tool reports a proxy 403, run once with `RUST_LOG=debug`. The proxy
prints only the denied destination host, never the secret or request headers.
Add that host either to the relevant secret's `hosts` (if it must receive the
credential) or to `egress_hosts` (if it must not).
