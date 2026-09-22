# Personal Credentials

Personal credentials are account-scoped secrets stored on Stashbase infrastructure. They are available only in **remote Agent Proxy sessions** (`stashbase agent run --remote`). The CLI never fetches, prints, exports, or stores their values locally.

## What They Are

Personal credentials belong to your authenticated account. Each profile that uses them declares `[personal_credentials.<NAME>]` bindings. The credential itself remains private to Stashbase servers—your local environment receives only an opaque placeholder that the remote proxy exchanges for the real value.

## Remote Session Behavior

When you run with `--remote`, the CLI:
1. Authenticates to Stashbase
2. Creates a scoped session token (memory-only, short-lived)
3. Provides the session token to a local relay proxy
4. The relay passes placeholder names to the child; the remote proxy resolves them

The real credential value never leaves Stashbase servers. The session token is revoked when the child exits.

## Profile Example

```toml
# .stashbase/agents/remote-example.toml
egress_hosts = ["mcp.linear.app"]

[personal_credentials.LINEAR_API_KEY]
env = "LINEAR_API_KEY"

[[personal_credentials.LINEAR_API_KEY.rules]]
effect = "allow"
hosts = ["mcp.linear.app"]
methods = ["GET", "POST"]
paths = ["/mcp"]
```

Run with:

```bash
stashbase agent run --remote --profile remote-example -- <your-tool>
```

The child receives `LINEAR_API_KEY=<placeholder>`. When it sends a request to `mcp.linear.app/mcp`, the remote proxy injects the real personal credential—if the policy allows it.

## Combining with Shared Secrets

A single profile can mix shared secrets (stored in Stashbase projects) and personal credentials:

```toml
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
paths = ["*"]

[personal_credentials.LINEAR_API_KEY]
env = "LINEAR_API_KEY"

[[personal_credentials.LINEAR_API_KEY.rules]]
effect = "allow"
hosts = ["mcp.linear.app"]
methods = ["GET", "POST"]
paths = ["/mcp"]
```

Both placeholders are injected according to their respective policies. Shared secrets still work locally; personal credentials require `--remote`.

## Format-Compatible Placeholders

Some tools validate an API-key shape before sending requests. Use a compatibility placeholder:

```toml
[personal_credentials.OPENAI_API_KEY]
env = "OPENAI_API_KEY"
header = "Authorization"
value_template = "Bearer {value}"
placeholder = "sk-proj-stashbase-placeholder-local-only"

[[personal_credentials.OPENAI_API_KEY.rules]]
effect = "allow"
hosts = ["api.openai.com"]
methods = ["POST"]
paths = ["/v1/*"]
```

The child sees the safe placeholder locally; the remote proxy exchanges it for the real credential on requests matching the policy.

## Key Differences from Shared Secrets

| Aspect | Shared Secrets | Personal Credentials |
|--------|---|---|
| Storage | Stashbase project/environment | Account-only (private) |
| Local access | Yes (with `file` or project/environment) | No (remote-only) |
| Session type | Local or remote | Remote only |
| Rotation | Project admin controls | User controls |
| CLI exposure | Can be listed, exported, shown | Never visible locally |

For local development with a single account, shared secrets work fine. For CI/CD where the session is temporary and scoped, personal credentials add an extra privacy layer.
