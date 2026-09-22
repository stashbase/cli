# Credential Policies

HTTP credential policies control where secrets can be injected and how they are formatted in requests. Policies are defined using `rules` that specify the effect (allow/deny), hosts, HTTP methods, and URL paths.

Each rule specifies an effect (`allow` or `deny`), hosts, HTTP methods, and URL paths:

```toml
[secrets.API_KEY]
env = "API_KEY"

[[secrets.API_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET", "POST"]
paths = ["/v1/data", "/v1/data/*"]

[[secrets.API_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET"]
paths = ["/v1/status"]

[[secrets.API_KEY.rules]]
effect = "deny"
hosts = ["api.example.com"]
methods = ["DELETE"]
paths = ["/v1/*"]
```

Rules use these primitives:
- **`effect`**: `"allow"` or `"deny"`
- **`hosts`**: List of exact hosts or wildcard subdomains
- **`methods`**: HTTP methods in uppercase (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, etc.)
- **`paths`**: URL paths with `*` matching any sequence of path characters; query strings are ignored

All matching rules are evaluated before injection:

1. If any `deny` rule matches, the credential is not injected.
2. If any `allow` rule matches, the credential is injected.
3. If no `allow` rule matches, the credential is not injected.
4. If a secret has rules, at least one matching `allow` is required.

## Deny Precedence

A matching `deny` rule always blocks the credential, even if other `allow` rules match:

```toml
[[secrets.API_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["GET"]
paths = ["/repos/*"]

[[secrets.API_KEY.rules]]
effect = "deny"
hosts = ["api.example.com"]
methods = ["GET"]
paths = ["/repos/*/secrets/*"]
```

Here, `GET /repos/acme/app/secrets/key` is blocked by the deny rule, even though it matches the allow rule.

## Global Egress and Denial

Egress policy is separate from credential injection:

```toml
egress_hosts = ["api.github.com", "registry.npmjs.org"]
deny_hosts = ["api.stashbase.dev"]
```

- `egress_hosts`: Hosts the agent may connect to (applies to all credentials and egress-only traffic).
- `deny_hosts`: Hosts the agent may not connect to, even if `egress_hosts` includes them or uses `"*"`.

Credential rules do not widen egress—a secret with rules for `api.example.com` does not allow the agent to connect there unless `egress_hosts` explicitly allows it.

## Custom Headers and Value Templates

By default, credentials are injected as `Authorization: Bearer <value>`. Customize for different auth schemes:

```toml
[secrets.API_KEY]
env = "API_KEY"
header = "x-api-key"
value_template = "{value}"
```

This injects `x-api-key: <value>` instead.

```toml
[secrets.GITHUB_TOKEN]
header = "Authorization"
value_template = "Bearer {value}"
```

This is the default. The request is evaluated against the configured policy before any credential is injected. Custom headers and value templates determine how an allowed credential is represented in the forwarded request.

