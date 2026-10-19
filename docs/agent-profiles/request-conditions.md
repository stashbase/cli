# Request Conditions

Host, method, and path rules decide *where* a credential may go. Request conditions also decide based on *what the request sends*: the JSON body of an HTTP request, or the arguments of an MCP tool call. The same `POST /v1/messages` can save a draft or email a customer. The same MCP `save_issue` tool can create an issue or rewrite one. Conditions tell these cases apart.

Conditions are optional keys on rules you already write. A rule without them behaves exactly as before.

| Rule | Conditions on | Keys |
|---|---|---|
| `[[secrets.*.rules]]`, `[[personal_credentials.*.rules]]` | the JSON request body | `body`, `allow_fields` |
| `[[mcp_servers.*.tool_rules]]` | `params.arguments` of a `tools/call` | `arguments`, `allow_fields` |

Both use the same conditions and the same `allow_fields`. Everything below applies to both. Examples use `body`; for MCP, write `arguments` instead.

## Quick example

```toml
[secrets.MAIL_KEY]
env = "MAIL_KEY"

# The agent may save drafts to two recipients, with a subject and text, and nothing else.
[[secrets.MAIL_KEY.rules]]
effect = "allow"
hosts = ["api.mail.example"]
methods = ["POST"]
paths = ["/v1/messages"]
body = { send = false, to = ["support@acme.com", "ops@acme.com"] }
allow_fields = ["subject", "text"]
```

| Request body | Result | Why |
|---|---|---|
| `{"send": false, "to": ["support@acme.com"]}` | allowed | both conditions hold |
| `{"send": false, "to": ["support@acme.com"], "subject": "Hi", "text": "..."}` | allowed | `subject` and `text` are in `allow_fields` |
| `{"send": true, "to": ["support@acme.com"]}` | refused | `send` must be `false` |
| `{"send": false, "to": ["x@evil.com"]}` | refused | `x@evil.com` is not a listed recipient |
| `{"send": false, "to": []}` | refused | an empty array never matches a list |
| `{"to": ["support@acme.com"]}` | refused | `send` is missing |
| `{"send": false, "to": ["support@acme.com"], "bcc": ["x@evil.com"]}` | refused | `bcc` is not listed anywhere |
| `send=false&to=support@acme.com` (form body) | refused | only JSON bodies are read |

## Conditions

Each key in `body` is a field of the JSON object, and each value is one condition:

| Condition | Holds when the field |
|---|---|
| `send = false` (string, number, or bool) | equals the value, with the same JSON type |
| `to = ["a@acme.com", "b@acme.com"]` | equals one of the values; for an array field, is non-empty and every element is one of them |
| `amount = { min = 1, max = 5000 }` | is a number in the range; both bounds are inclusive and either may be omitted |
| `id = { absent = true }` | is missing or `null` |

Details:

- **Types are strict.** `5` is not `"5"`, and `true` is not `"true"`. Strings are compared exactly and case-sensitively.
- **Named fields must be present.** A field that is missing, `null`, or of another type fails every condition except `absent`.
- **A list means "one of these values", not "exactly this array".** `to = ["a", "b"]` accepts `"a"`, `["a"]`, `["b", "a"]`, and `["a", "a"]`. No condition requires an array to equal a fixed array.
- **Nested fields use dots.** `"destination.type" = "bank"` checks `{"destination": {"type": "bank"}}`. A path that runs into an array or a plain value fails. Array indexes (`items.0.amount`) are not supported.
- **Matching is exact only.** No wildcards or regular expressions.

## Combining rules

- **All conditions in one rule must hold.**
- **Separate rules are alternatives.** A request is allowed if any allow rule matches.
- **A matching deny always wins.**

Per-currency limits, for example, are two allow rules:

```toml
[[secrets.PAYMENTS_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]
body = { currency = "usd", amount = { min = 1, max = 5000 } }

[[secrets.PAYMENTS_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]
body = { currency = "eur", amount = { min = 1, max = 4000 } }
```

| Request body | Result |
|---|---|
| `{"currency": "usd", "amount": 4500}` | allowed by the first rule |
| `{"currency": "eur", "amount": 4500}` | refused: over the eur limit, and not usd |
| `{"currency": "gbp", "amount": 10}` | refused: neither rule matches |
| `{"currency": "usd", "amount": "4500"}` | refused: `amount` is a string |

**An allow rule without conditions makes conditions pointless.** If another allow rule for the same host, method, and path has no `body`, every request is allowed anyway. `stashbase agent validate` warns about this. For MCP, the same applies to tools listed in `allow_tools`, which allows them with any arguments.

## `allow_fields`

Conditions check only the fields they name, so other fields pass unchecked. `allow_fields` closes that gap. Once it is set, the body may contain only fields named in `body` or in `allow_fields`; any other field makes the rule fail.

| Rule | `{"send": false}` | `{"send": false, "subject": "x"}` | `{"send": false, "bcc": "x"}` |
|---|---|---|---|
| `body = { send = false }` | allowed | allowed | allowed |
| `body = { send = false }`, `allow_fields = ["subject"]` | allowed | allowed | refused |
| `body = { send = false }`, `allow_fields = []` | allowed | refused | refused |

Nested fields:

- **A listed field allows everything under it.** `allow_fields = ["meta"]` permits `meta.a`, `meta.b.c`, and so on.
- **A field that only leads to listed paths may contain just those.** With `"project.id"` in `body` or `allow_fields`, `{"project": {"id": "P"}}` passes but `{"project": {"id": "P", "owner": "x"}}` is refused for `project.owner`.

`allow_fields` is for allow rules only. A deny rule cannot limit which fields a request has, and validation rejects it.

## Recipes

**Require one value, allow anything else:**

```toml
body = { send = true }
```

**An exact body.** Put a condition on every field and allow no others. Whitespace and key order don't matter; the content does.

```toml
body = { send = true, to = ["support@acme.com"], subject = "Daily report" }
allow_fields = []
```

**Create but not update.** Many tools and APIs create without an `id` and update with one:

```toml
[[mcp_servers.linear.tool_rules]]
effect = "allow"
tools = ["save_issue"]
arguments = { team = ["Engineering"], id = { absent = true } }
allow_fields = ["title", "description", "priority"]
```

**A carve-out from a broad allow.** Allow everything on an endpoint except one kind of request:

```toml
[[secrets.PAYMENTS_KEY.rules]]
effect = "allow"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]

[[secrets.PAYMENTS_KEY.rules]]
effect = "deny"
hosts = ["api.example.com"]
methods = ["POST"]
paths = ["/v1/payouts"]
body = { "destination.type" = "card" }
```

A deny with conditions is best-effort: an agent can often reach the same result with a different body, for example through another field or endpoint. `stashbase agent validate` warns about it. Prefer allow rules that name what is permitted.

## How the proxy reads the body

For HTTP rules:

1. The proxy matches host, method, and path as usual. If no matching rule for the credential has `body` or `allow_fields`, the request is streamed without its body being read.
2. Otherwise the proxy holds the request, reads the body, and only then decides whether to inject the credential.
3. If allowed, the upstream receives exactly the bytes that were checked; the body is never re-encoded.

A body is readable only when all of these hold:

| Requirement | Otherwise |
|---|---|
| `Content-Type` is `application/json` or `application/*+json` | `body_unreadable:content_type` |
| no `Content-Encoding` (or `identity`) | `body_unreadable:encoding` |
| at most 1 MiB | `body_unreadable:size` |
| valid JSON whose top level is an object | `body_unreadable:json` |
| no key repeated at any depth | `body_unreadable:duplicate_key` |

An unreadable body, including no body, **fails closed**: allow rules with conditions don't match, and deny rules with conditions do. So a malformed, compressed, oversized, or non-JSON body can neither satisfy an allow nor get around a deny. Repeated keys are refused because parsers disagree on which value wins, and the proxy must read the request the same way the upstream does.

For MCP rules the proxy already reads every request to a configured MCP endpoint. Missing `params.arguments` counts as an empty object. Arguments that aren't an object fail closed, the same way an unreadable body does. JSON-RPC batch requests and repeated keys are rejected.

## Denials and audit

A refused request gets a 403 naming the rule and field, never a value:

```
The supplied credential is not allowed for this request (rule 1: body.send).
The supplied credential is not allowed for this request (rule 1: unlisted body.bcc).
MCP tool arguments are not allowed by the agent profile (arguments.team)
```

When several allow rules fail, the message names the first one. The audit log records a reason code:

| Reason | Meaning |
|---|---|
| `allow_condition_failed:body.send` | no allow rule matched; this field failed first |
| `unlisted_field:body.bcc` | the field is neither in the conditions nor in `allow_fields` |
| `deny_condition_matched:body.destination.type` | a deny rule with conditions matched |
| `body_unreadable:json` | the body could not be read (see the table above) |

MCP reasons use `arguments.` instead of `body.`. Filter by reason with `stashbase agent logs list --reason allow_condition_failed`; a bare code also matches the versions that name a field.

## Testing your rules

**Policy tests** check rules without sending requests. A test's `body` is JSON text:

```toml
[[policy_tests]]
name = "sending is refused"
secret = "MAIL_KEY"
method = "POST"
host = "api.mail.example"
path = "/v1/messages"
body = '{"send": true, "to": ["support@acme.com"]}'
expect = "deny"
```

```
$ stashbase agent policy test --profile mail
✓ sending is refused: expected deny, got deny (no credential allow rule matches, rule 1: body.send)
```

A test without `body` is evaluated like a request without one, so it never meets conditions.

Other tools:

- **`stashbase agent validate`** rejects malformed conditions and warns about pointless or best-effort rules.
- **`stashbase agent explain`** has no body to look at, so it reports such requests as "eligible only if the request body meets the rule's body conditions".
- **`stashbase agent mcp tools --profile <name> --server <server> --tool <tool>`** lists a tool's arguments, types, and the rules for it. It warns about fields in `arguments` or `allow_fields` that the tool doesn't declare: a misspelled field never matches, so its allow rule refuses every call.

[examples/http-body-conditions.toml](./examples/http-body-conditions.toml) runs against httpbin.org with no real key. [examples/linear-tool-rules.toml](./examples/linear-tool-rules.toml) shows MCP argument conditions for Linear.

## Limits

- **Single requests only.** Conditions don't limit totals: an agent allowed 5000 per payout can make several payouts.
- **JSON only.** Form-encoded and multipart bodies are unreadable, and query strings are not checked.
- **No array element conditions**, such as "every `items[].amount` is at most 100".
- **`--remote`:** the remote proxy enforces the same conditions, for MCP `arguments` and HTTP `body`, with the same messages and audit reasons.
