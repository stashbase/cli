# Integration Examples

Quick-start profiles for common tools and APIs.

## Claude Code

Claude Code can access external tools and APIs through agent profiles. For example, to grant Linear MCP server access:

```toml
# .stashbase/agents/claude.toml
egress_hosts = ["api.anthropic.com", "mcp.linear.app"]

[secrets]
project = "my-project"
environment = "development"

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
allow_tools = ["search_issues", "get_issue", "create_issue"]
```

Run:

```bash
stashbase agent run --profile claude -- claude
```

Claude Code receives only the Linear credential placeholder and can use allowed MCP tools through the proxy.

## Codex

Codex needs GitHub access for repository operations and OpenAI for completions:

```toml
# .stashbase/agents/codex.toml
egress_hosts = ["api.openai.com", "chatgpt.com", "api.github.com"]
allow_hooks = ["dependency_check"]

[secrets]
project = "my-project"
environment = "development"

[secrets.GITHUB_TOKEN]
env = "GH_TOKEN"

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*/*", "/repos/*/*/issues*", "/repos/*/*/pulls*"]

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["POST"]
paths = ["/repos/*/*/issues", "/repos/*/*/issues/*/comments"]

[secrets.OPENAI_API_KEY]
env = "OPENAI_API_KEY"

[[secrets.OPENAI_API_KEY.rules]]
effect = "allow"
hosts = ["api.openai.com"]
methods = ["POST"]
paths = ["/v1/*"]
```

Run:

```bash
stashbase agent run --profile codex -- codex
```

## HTTP MCP Server

MCP servers over HTTP (like Linear) use a separate credential binding and tool allowlist:

```toml
# .stashbase/agents/linear-mcp.toml
egress_hosts = ["mcp.linear.app"]

[secrets]
project = "my-project"
environment = "development"

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
allow_tools = ["search_issues", "get_issue", "create_issue"]
```

The HTTP rule controls credential injection at the endpoint. The MCP server entry controls which tools are available. Both are required.

Inspect available tools:

```bash
stashbase agent mcp tools --profile linear-mcp --server linear
```

Update allowed tools interactively:

```bash
stashbase agent mcp configure --profile linear-mcp --server linear
```

## Generic HTTP API

For any HTTP API that uses a Bearer token:

```toml
# .stashbase/agents/generic-api.toml
egress_hosts = ["api.example.com"]

[secrets]
project = "my-project"
environment = "development"

[secrets.API_KEY]
env = "API_KEY"
header = "Authorization"
value_template = "Bearer {value}"

[[secrets.API_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET", "POST"]
paths = ["/v1/*"]
```

For a custom header:

```toml
[secrets.API_KEY]
env = "API_KEY"
header = "x-api-key"
value_template = "{value}"

[[secrets.API_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET", "POST"]
paths = ["/v1/*"]
```

## Multiple Credentials in One Profile

Combine several independent secrets in a single profile:

```toml
# .stashbase/agents/full-stack.toml
egress_hosts = ["registry.npmjs.org", "api.github.com", "api.openai.com"]

[secrets]
project = "my-project"
environment = "development"

[secrets.NPM_TOKEN]
env = "NPM_TOKEN"
header = "Authorization"
value_template = "Bearer {value}"

[[secrets.NPM_TOKEN.rules]]
effect = "allow"
hosts = ["registry.npmjs.org"]
methods = ["GET"]
paths = ["/-/npm/*"]

[secrets.GITHUB_TOKEN]
env = "GH_TOKEN"

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*/*"]

[secrets.OPENAI_API_KEY]
env = "OPENAI_API_KEY"

[[secrets.OPENAI_API_KEY.rules]]
effect = "allow"
hosts = ["api.openai.com"]
methods = ["POST"]
paths = ["/v1/*"]
```

Each secret has its own source binding, rules, and header format. The child receives only placeholders, never real values.

## Local Overrides

Mix shared secrets with a local `.env` file:

```toml
# .stashbase/agents/dev.toml
file = ".env.local"

[secrets]
project = "my-project"
environment = "development"

[secrets.GITHUB_TOKEN]
env = "GH_TOKEN"

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET", "POST"]
paths = ["*"]

[secrets.OPENAI_API_KEY]
env = "OPENAI_API_KEY"

[[secrets.OPENAI_API_KEY.rules]]
effect = "allow"
hosts = ["api.openai.com"]
methods = ["POST"]
paths = ["/v1/*"]
```

If `GITHUB_TOKEN` exists in `.env.local`, Stashbase uses it; otherwise it fetches from the project. If `OPENAI_API_KEY` is not in `.env.local`, Stashbase fetches it. The child receives placeholders for both.

## Personal Credentials Example

Personal credentials are private to your account and available only in remote sessions:

```toml
# .stashbase/agents/full-access.toml
egress_hosts = ["api.github.com", "mcp.linear.app"]

[secrets]
project = "my-project"
environment = "development"

[secrets.GITHUB_TOKEN]
env = "GH_TOKEN"

[[secrets.GITHUB_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET", "POST"]
paths = ["/repos/*/*"]

[personal_credentials.LINEAR_API_KEY]
env = "LINEAR_API_KEY"

[[personal_credentials.LINEAR_API_KEY.rules]]
effect = "allow"
hosts = ["mcp.linear.app"]
methods = ["GET", "POST"]
paths = ["/mcp"]

[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
binding = "LINEAR_API_KEY"
allow_tools = ["search_issues", "get_issue"]
```

Run with `--remote`:

```bash
stashbase agent run --remote --profile full-access -- <your-tool>
```

The profile mixes a shared GitHub token (from project/environment) with your personal Linear credential. Both are injected as placeholders; the real values never leave Stashbase servers.

## Testing Policies Locally

Before using a profile in CI, validate and explain policies:

```bash
stashbase agent validate --profile my-profile
stashbase agent explain --profile my-profile \
  --host api.github.com --method GET --path /repos/acme/widget/issues
stashbase agent profiles show my-profile --effective
```

These commands never load secret values or start a proxy.
