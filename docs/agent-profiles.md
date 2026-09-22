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

For complete network isolation, use a container or VM.

## Full Reference

This is a lightweight local guide. For comprehensive documentation on advanced features, policy regression tests, compatibility matrices, and operational details, see https://docs.stashbase.dev/agents.
