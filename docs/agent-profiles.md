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

## Filesystem, Network Restrictions, and Sandbox Backends

Restrict what the child can read or write:

```toml
[filesystem]
deny_read = ["~/.ssh", "~/.aws"]
deny_write = ["~/.git"]
```

Paths use explicit prefixes: `~` for home, relative paths for the current directory. A denied directory covers everything under it.

Entries can also be patterns:

```toml
[filesystem]
deny_read = [
  "**/.env*",               # glob: any .env file at any depth below the current directory
  "~/.config/*/token",      # `*` and `?` match within one path segment
  "re:^~/.*\\.pem$",        # regex, prefixed `re:`, matched against the absolute path
]
```

- An entry containing `*` or `?` is a glob. `**` matches any number of directories, and `[a-z]`/`[!.]` character classes work inside globs. Brackets on their own don't make an entry a glob, so a plain path like `~/notes/[old]` keeps its literal meaning.
- An entry prefixed `re:` is a regular expression. It must start with `^/` or `^~/`, and it may only use POSIX constructs: write `[0-9]` instead of `\d`, and group alternatives (`^/(a|b)/...`) instead of using a top-level `|`.
- `path:` forces a literal path, e.g. `path:/data/what?`.
- Like a plain path, a pattern that matches a directory denies everything under it. This holds for regexes ending in `$` too: `re:^/srv/secrets$` also denies `/srv/secrets/key`.
- On macOS, the native backend matches patterns every time a file is accessed, so files the agent creates during the run are covered. Linux and the Docker backend expand patterns once, when the run starts; files created later aren't covered. `stashbase agent validate` warns about this, and `stashbase agent explain` lists each pattern with how it's matched.
- Patterns aren't supported on Windows; validation fails.

By default, enforcement uses the platform-native mechanism (Seatbelt on macOS, `systemd-run`/`bubblewrap` on Linux). **If Docker is available, prefer `[sandbox] backend = "docker"` instead** — it's meaningfully stronger (allow-list filesystem access, a real network-layer firewall, and it works on Windows too, unlike the native backend).

See **[Sandboxing](sandboxing.md)** for the full picture: both backends, how the Docker backend's network firewall is enforced, custom images, git identity forwarding, login persistence, and Codex/Claude Code OAuth quirks.

## Worktrees

Run the agent in its own git worktree (branch `stashbase/<name>`, inside the repo at `.stashbase/worktrees/<name>`) instead of your checkout, so several agents can work in one repository in parallel:

```toml
[workspace]
worktree = true
```

Or per run: `stashbase agent run --profile coding --worktree -- claude` (`--worktree=false` skips it when the profile turns it on). Works with the native backend and, on macOS/Linux, the Docker backend. Review and merge the agent's work from your checkout with `stashbase agent worktrees list`, `merge <name>` and `clean` — see **[Worktrees](sandboxing.md#worktrees)**.

## Config Environment Variables

Set non-sensitive configuration for the agent with `[config_env]`. Values are stored in plaintext in the profile, and both sandbox backends apply them. The Docker backend otherwise starts the container with an empty environment. A value can be a string, a number or a boolean (`PORT = 3000` and `DEBUG = true` work without quotes), and the agent always receives it as a string.

```toml
[config_env]
NODE_ENV = "development"
RUST_LOG = "info"
DATABASE_URL = "postgres://app@localhost:5432/app"
```

- `[config_env]` values take precedence over variables inherited from your shell. They can't replace anything the sandbox sets: proxy and CA variables, `HOME`, `TMPDIR`, `FORCE_COLOR` and anything starting with `STASHBASE_` are rejected. `PATH`, `LD_*` and `DYLD_*` are rejected too, because they control which programs and libraries start the sandbox.
- Each `NAME=value` can be up to 64 KiB, and the whole table up to 256 KiB. That keeps `[config_env]` well within what the operating system accepts when starting the agent. Values can't contain NUL bytes. Keep larger content in a file in the workspace.
- A name that a `[secrets]` or `[personal_credentials]` binding already provides (its `env` name, or a renamed `from` source) is rejected.
- Sensitive values don't belong here. Stashbase checks every value: a sensitive-looking name (`*TOKEN*`, `*PASSWORD*`, `*API_KEY*`, …), a known token format, a URL containing a password, or a long random-looking string triggers a warning in `agent validate`. `agent run` then asks for confirmation before starting, and fails when it can't ask (non-interactive or `--silent`). Flagged values are never printed: `agent profiles show` displays them as `<hidden: looks sensitive>`.
- If a flagged value really isn't secret, acknowledge it so the check skips it:

  ```toml
  [config_env]
  BUILD_HASH = { value = "9f86d081884c7d659a2feaa0c55ad015", not_sensitive = true }
  ```

To set or override a variable for one run without editing the profile, pass `--config-env NAME=VALUE` (repeatable):

```bash
stashbase agent run --profile coding --config-env NODE_ENV=test --config-env PORT=4000 -- claude
```

Flag values go through the same checks as the profile's, including the sensitive-value check: a value that looks sensitive asks for confirmation, or fails the run when it can't ask.

The audit log's `session_started` event lists the names of the variables set for the run (`config_env_names`), never their values.

A complete example profile is in [examples/config-env.toml](agent-profiles/examples/config-env.toml). It includes commented-out lines that show the warning and the validation errors.

## Inherited Shell Variables

The native backend passes your shell's environment to the agent (only `STASHBASE_API_KEY` is removed). A token you exported, like `GITHUB_TOKEN` or `OPENAI_API_KEY`, reaches the agent as a real value and skips the placeholder model. The Docker backend doesn't have this problem: the container gets only the variables the sandbox passes explicitly, so the check below is skipped there.

At startup, a native `agent run` (local or `--remote`) prints one line with how many credential-looking variables the agent would inherit, and `--silent` hides it. `stashbase agent validate --profile <name>` lists them by name. Values are never printed. To keep a variable from the agent, blank it in the profile, bind it under `[secrets]` if the agent needs it, or use the Docker backend:

```toml
[config_env]
GITHUB_TOKEN = ""
```

`[config_env]` overrides inherited variables, and an empty value isn't flagged as sensitive.

The check is best-effort. A name matches if it contains `TOKEN`, `SECRET`, `PASSWORD`, `PASSWD`, `API_KEY`, `APIKEY`, `PRIVATE_KEY` or `CREDENTIAL`. A value matches if it has a known token format (`ghp_`, `sk-`, `AKIA`, ...) or is a URL with a password. Unlike the `[config_env]` check, random-looking values aren't flagged, because a shell environment has too many session ids and hashes. Skipped: values that are paths (`GH_TOKEN_FILE=/run/secrets/gh`), names the sandbox sets, and names a binding or `[config_env]` already provides. A credential with an unusual name, or a capability like `SSH_AUTH_SOCK`, isn't recognized. Use `[filesystem] deny_read` for credential files.

## Network Access and HTTP Rules

By default, the proxy denies all connections. Allow specific destinations:

```toml
egress_hosts = ["api.github.com", "registry.npmjs.org"]
```

`egress_hosts` covers requests that don't carry a credential. A host named in a secret's [credential rules](#http-credential-rules) doesn't need to be listed: the proxy lets through requests that carry that secret and match its rules, and blocks other requests to the host. List a credential host in `egress_hosts` too only when the tool also calls it without the credential.

Use `"*"` for unrestricted internet (but this does not bypass credential rules—each secret still honors its own policy).

Optional: block specific hosts even when egress is broad:

```toml
egress_hosts = ["*"]
deny_hosts = ["api.stashbase.dev"]
```

### Credentials your tools hold themselves

The proxy recognizes a credential only by its Stashbase placeholder. Any other credential a tool uses is forwarded like a request without one: wherever `egress_hosts` allows the host (everywhere with `"*"`), with no secret rules and no secret named in the audit log. Where such credentials come from:

- **Docker:** only from inside the sandbox — a login saved in the [sandbox home](sandboxing.md#login-persistence) (`gh auth login`, `aws configure`), a custom image, or files in the working directory. Your host's environment and home directory aren't passed in.
- **Native:** also from your machine — exported shell variables (`GH_TOKEN`, `AWS_*`, …), credential files in your home directory (`~/.config/gh`, `~/.aws`) unless listed in `deny_read`, and your keychain (`gh` on macOS). SSH keys don't go through the proxy at all; deny `~/.ssh` to keep them out.

The coding agents' own logins work this way too, which is why Claude Code and Codex need their API and login hosts in `egress_hosts` (see [Codex and subscription login](sandboxing.md#codex-and-subscription-login)).

To keep a tool's access under the profile, bind its credential as a secret (e.g. `GH_TOKEN` for `gh`), and prefer the Docker backend for a clean environment.

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

### Body conditions

See [request-conditions.md](./agent-profiles/request-conditions.md) for the full guide to body and argument conditions, with examples.

The same `POST` can save a draft or send to a customer. An optional `body` on a rule matches JSON request bodies, with the same conditions as [MCP argument conditions](#argument-conditions):

```toml
[[secrets.MAIL_KEY.rules]]
effect = "allow"
hosts = ["api.mail.example"]
methods = ["POST"]
paths = ["/v1/messages"]
body = { send = false, to = ["support@acme.com", "ops@acme.com"] }

[[secrets.PAYMENTS_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]
body = { currency = "usd", amount = { min = 1, max = 5000 }, "destination.type" = "bank" }
```

`allow_fields` works the same way as for MCP rules: with it set, the body may contain only fields named in `body` or `allow_fields`, and others are refused with `reason=unlisted_field:body.<field>`. A rule with only `allow_fields` also reads the body.

Policy tests can send a body, so these rules can be checked without a request:

```toml
[[policy_tests]]
secret = "MAIL_KEY"
method = "POST"
host = "api.mail.example"
path = "/v1/messages"
body = '{"send": true, "to": ["support@acme.com"]}'
expect = "deny"
```

The body is read as JSON. A case without `body` is evaluated like a request without one, so it never meets body conditions. See [examples/http-body-conditions.toml](./agent-profiles/examples/http-body-conditions.toml) for a profile you can run against httpbin.org without a real key.

A rule without `body` behaves as before, and the proxy streams the request without reading it. When a credential has a rule with `body` that matches the host, method and path, the proxy reads the body before injecting the credential and forwards exactly the bytes it checked.

The body is readable only when `Content-Type` is `application/json` or `application/*+json`, there is no `Content-Encoding`, it is at most 1 MiB, and it is a JSON object without repeated keys. Any other body, including no body, fails closed: allow rules with `body` don't match and deny rules with `body` do.

A refused request returns 403 naming the rule and field, for example `The supplied credential is not allowed for this request (rule 1: body.send)`, and the audit log records `reason=allow_condition_failed:body.send` (or `deny_condition_matched:…`, `body_unreadable:encoding`), never the values. `stashbase agent validate` warns when an allow rule without `body` makes body conditions moot and that deny rules with `body` are best-effort. `stashbase agent explain` reports a request decided only by body conditions as eligible only if the body meets them. The remote proxy (`--remote`) enforces the same conditions.

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
allow_tools = ["list_issues", "get_issue"]
```

The HTTP rule controls credential injection at the endpoint. The MCP rule controls which tools the agent may call. Omit `allow_tools` or leave it empty to deny all tools by default. Use `allow_tools = ["*"]` to allow all tools (a matching `deny_tools` still takes precedence).

### Argument conditions

See [request-conditions.md](./agent-profiles/request-conditions.md) for the full guide, shared with HTTP body conditions.

A tool name often isn't enough: the same tool can draft or send, create or update. `[[mcp_servers.*.tool_rules]]` allows or denies a tool only when its call arguments match:

```toml
[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
binding = "LINEAR_API_KEY"
allow_tools = ["list_issues", "get_issue"]

# Create issues in one team only. save_issue also updates when given an id.
[[mcp_servers.linear.tool_rules]]
effect = "allow"
tools = ["save_issue"]
arguments = { team = ["Engineering"], id = { absent = true } }
```

Each key under `arguments` is a field of the call's `params.arguments`, with dots for nested objects (`"project.id"`). Each value is one condition:

| Condition | Holds when the field |
|---|---|
| `mode = "draft"` (string, number or bool) | equals the value, with the same JSON type (`"5"` is not `5`) |
| `to = ["a@acme.com", "b@acme.com"]` | is one of the values; for an array field, is non-empty and every element is one of them |
| `amount = { min = 1, max = 5000 }` | is a number in the range; both bounds are inclusive and optional |
| `id = { absent = true }` | is missing or `null` |

All conditions in a rule must hold, separate rules are alternatives, and a matching deny always wins. `allow_tools` and `deny_tools` are tool rules without conditions, so a tool listed in `allow_tools` (or covered by `allow_tools = ["*"]`) is allowed with any arguments and its conditioned allow rules have no effect. `stashbase agent validate`, `agent mcp tools` and `agent mcp configure` warn when that happens. A field that is missing, `null` or of another type fails every condition except `absent`.

Arguments that aren't a JSON object fail closed: allow rules with conditions don't match and deny rules with conditions do. The proxy also rejects JSON-RPC batch requests and request bodies that repeat a key, so it never decides on a different reading of the request than the MCP server.

Conditions only check the fields they name, so other fields pass unchecked. To allow nothing else, list the remaining fields in `allow_fields`; any other field fails the rule (`reason=unlisted_field:arguments.bcc`):

```toml
[[mcp_servers.mail.tool_rules]]
effect = "allow"
tools = ["send_message"]
arguments = { to = ["support@acme.com"] }
allow_fields = ["subject", "text"]   # nothing besides to, subject and text
```

A listed field allows everything under it (`"meta"` allows `meta.anything`). A field that only leads to listed paths may contain just those: with `"project.id"` listed, `project.owner` is refused. `allow_fields = []` allows only the fields with conditions. `allow_fields` is for allow rules; deny rules reject it.

Prefer allow rules with conditions. A deny with conditions is best-effort, because an agent can often reach the same result with other arguments, and `stashbase agent validate` warns about it. Conditions check single calls; they don't limit how many calls the agent makes.

A refused call returns 403 with the field that decided it, for example `MCP tool arguments are not allowed by the agent profile (arguments.team)`. The audit log records the same as `reason=allow_condition_failed:arguments.team`, never the argument values. `stashbase agent mcp check` shows whether a tool is allowed only under conditions. The remote proxy (`--remote`) enforces the same conditions.

Argument names come from the MCP server, and a misspelled field never matches, so its allow rule denies every call. `stashbase agent mcp tools --profile <name> --server <server> --tool <tool>` lists the tool's arguments (dotted paths, types, required, allowed values) and the profile's rules for it. Without `--tool`, the command warns about rule fields a tool doesn't declare and about tools the server doesn't offer. See [examples/linear-tool-rules.toml](./agent-profiles/examples/linear-tool-rules.toml).

## API Hooks

Hooks that call the Stashbase API (dependency checking, secret scanning) must be granted explicitly:

```toml
allow_hooks = ["dependency_check", "secret_scan"]
```

The child receives only a scoped local broker token, never your API key. Hooks are disabled by default.

`dependency_check` works with the hooks from `stashbase agent hooks deps install`. With the native backend they run your installed CLI, which asks the Agent Proxy to call the API. The Docker image has no Stashbase CLI, so in a Docker run a small stand-in is mounted as `stashbase` inside the container: it forwards the hook's input to the Agent Proxy, which runs `stashbase agent hooks` on the host, confined the same way as the secret scan below, and returns its answer. If the proxy can't be reached, the stand-in blocks the command instead of letting the install through unchecked. In a Docker run the agent can't see the global hook config in your home directory. Either install the hook per repository (without `--global`; the container sees the repo), or install it into the Docker sandbox's own home, which every Docker run shares: `stashbase agent hooks deps install claude --docker` (likewise `check` and `uninstall`).

`secret_scan` lets the git hooks from `stashbase scan install` run inside the sandbox. The hook sends only a token to the Agent Proxy, which runs `stashbase scan staged` (or `unpushed`) on the host, in the run's working directory, with your key. Because the agent can edit the repo's scan config, these scans ignore its `match` and `output-dir` settings; `excluded-files` and `ignored-secrets` still apply. The host-side scan is confined, so a symlink or a `.git` redirect planted by the agent can't point it at your other files. On Linux it sees only system files plus the run's working tree, git directories and the CLI binary. On macOS it can't read file contents anywhere users' data lives (home directories, `/Volumes`, `/tmp`, `/var/folders`, `/opt`) except those same run paths; system files stay readable, and file names and sizes stay visible because path lookup needs them. The profile's `deny_read` entries stay denied on both. This uses Seatbelt on macOS and bubblewrap on Linux; where neither is available (including Windows), a profile with `secret_scan` refuses to start. Restricted scans also stop at 1 MiB per changed file, 32 MiB in total, 10,000 files or 1,000 commits; larger changes have to be committed from outside the sandbox. The hook needs `curl` in the sandbox (the default Docker image has it). Hooks installed before this existed need `stashbase scan install` once more to pick up the sandbox-aware block. Without `secret_scan`, a scan hook inside the sandbox fails with a message pointing here.

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

For stronger filesystem and network isolation than the native backend provides, see [Sandboxing](sandboxing.md).

## Full Reference

This is a lightweight local guide. For comprehensive documentation on advanced features, policy regression tests, compatibility matrices, and operational details, see https://docs.stashbase.dev/agents.
