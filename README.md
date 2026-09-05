# Stashbase CLI

The Stashbase CLI is the official command-line tool for the Stashbase secrets management platform for developers.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [License](#license)
- [Contact](#contact)

## Installation

You have multiple options to install the Stashbase CLI, either via package managers, shell script or you can just download the binary from the [releases](https://github.com/stashbase/cli/releases) page directly.

Beta platform support:
- macOS Apple Silicon
- Linux x64
- Windows x64

Intel macOS is currently not supported.

### macOS

Stashbase CLI is available via [Homebrew](https://brew.sh) for macOS Apple Silicon users.

```bash
# Add the tap
brew tap stashbase/homebrew-stashbase

# Trust the tap
brew trust stashbase/stashbase

# Install the CLI
brew install stashbase
```

### Linux

For Linux x64 users, we recommend downloading Stashbase CLI using shell script.

```bash
curl -fsSL https://stashbase.dev/cli/install.sh | bash
```

### Windows

For Windows x64 users, we recommend using [Scoop](https://scoop.sh).

```bash
# Add the bucket
scoop bucket add stashbase https://github.com/stashbase/scoop-stashbase

# Install the CLI
scoop install stashbase
```

## Usage

For full documentation, please visit [Stashbase CLI Documentation](https://docs.stashbase.dev/cli).

### First-time setup
You can run the interactive setup command to configure the CLI for the first time.

```bash
stashbase setup
```

Setup asks for a profile name and pre-fills `default`; press Enter to keep the
standard one-workspace setup.

### Authenticate with Stashbase
If you don't set the API Key during setup, you can set it later manually.
You can generate an API Key in your Stashbase workspace by going to API Keys -> Personal API Keys -> Create API Key.

```bash
# interactively set API key
stashbase config api-key set

# or set API key via stdin (e.g. from environment variable or secret manager)
printf '%s' 'sb_personal_35tnv...' | stashbase config api-key set --stdin
```

### Credential profiles

Use profiles when you work with more than one Stashbase workspace. Each
profile's API key is stored separately in the OS secure credential store; the
config file contains only the profile name and optional workspace label.

```bash
# Create a profile and enter its API key when prompted.
stashbase config profile add acme --workspace acme-production

# Make it your usual profile.
stashbase config profile use acme

# Use a profile for one command or an automation run.
STASHBASE_PROFILE=acme stashbase projects list
```

Selection uses `STASHBASE_PROFILE`, then the configured default profile (or the
backwards-compatible `default` profile). `--api-key` and `STASHBASE_API_KEY`
override the selected profile's stored key, which is useful in CI.

### API key storage

`stashbase config api-key set` stores your API key in the OS secure credential store:
- macOS: Keychain
- Linux: Secret Service (`secret-tool`)
- Windows: DPAPI-encrypted local secret file

The CLI config file is now used for non-sensitive settings and is written with owner-only permissions on Unix systems.
If secure storage is unavailable, the CLI falls back to config-file storage and prints a warning.

### List projects

```bash
stashbase projects list
```

### List environments

```bash
stashbase environments list -p <PROJECT>
```

### List secrets

```bash
stashbase secrets list -p <PROJECT> -e <ENVIRONMENT>
```

### Export a safe agent environment schema

```bash
stashbase secrets schema pull --project api --environment production
```

This writes `env.schema.yaml` with the selected project and environment metadata (including their IDs), plus secret names and comments; it never writes secret values or secret IDs. Pass `--output <PATH>` to choose another location.

### Run commands with injected secrets

```bash
# load config from stashbase.yaml and select a config entry interactively
stashbase run -- npm run dev

# load secrets from Stashbase and run a command
stashbase run -p <PROJECT> -e <ENVIRONMENT> -- npm run dev

# load secrets from a local env file and run a command
stashbase run --file .env.production -- npm run dev

# local file input supports dotenv, yaml/yml, and json
stashbase run --file secrets.yaml -- npm run dev
```

### Agent Credential Proxy (Experimental)

`run --proxy` starts an in-process, localhost-only HTTP proxy for the lifetime
of the child command. Instead of receiving the loaded secret, the child receives
a placeholder such as `**STASHBASE_GH_TOKEN**`. When the child sends that value
as an `Authorization: Bearer` header, the proxy replaces it before forwarding
the request. It rewrites headers only: request and response bodies stream
through unchanged, including chunked uploads, downloads, and SSE responses.

```bash
stashbase run --proxy --only GH_TOKEN -- gh workflow run deploy.yml
```

The proxy prints its temporary localhost port when it starts and stops as soon
as the child command finishes. It is not a daemon and does not write credentials
to stdout or logs.

It chooses a random localhost port by default. For debugging or a local
integration that requires a stable port, use `--proxy-port`:

```bash
stashbase agent run --proxy-port 8787 --profile coding -- codex

# The regular run command supports it too.
stashbase run --proxy --proxy-port 8787 --only GH_TOKEN -- gh auth status
```

The requested port must be available and between 1 and 65535.

This is a feasibility experiment, not a production credential boundary. HTTPS
rewriting requires TLS interception, so the proxy creates a temporary local CA
and provides its path through standard child-process trust variables
(`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, and `GIT_SSL_CAINFO`). `curl` can use this
on typical systems. A client that ignores these variables, pins certificates,
uses HTTP/2-only proxy traffic, or bypasses proxy environment variables will
not work; in particular, `gh` may not trust the temporary CA on every platform.
For `run --proxy`, only exact `Authorization: Bearer <placeholder>` headers
are rewritten. Agent profiles can additionally configure a provider-specific
HTTP header; non-HTTP traffic and approval flows remain out of scope.

Node's built-in `fetch` is configured through `NODE_USE_ENV_PROXY=1` and
`NODE_EXTRA_CA_CERTS`, which the proxy supplies automatically.

For safe troubleshooting, set `RUST_LOG=debug`. Proxy diagnostics identify
only the denied or unreachable destination host; they never include headers or
secret values.

### Agent profiles (experimental)

> **Early access — local exposure reduction, not hostile-agent isolation.**
> `agent run` keeps profile secrets out of the child environment and proxies
> them only to configured HTTP(S) destinations. It does not prevent a
> malicious same-user process from accessing the developer's broader Stashbase
> credentials or bypassing the intended workflow. The remote `only` parameter
> limits this CLI request; it is not server-enforced authorization for a holder
> of a normal personal or service API key.

For a repository-local starting point, create a deliberately closed profile,
then replace the placeholder and example rule with the capability your agent
needs:

```bash
stashbase agent init codex
stashbase agent validate --profile codex --profile-source directory
stashbase agent run --profile codex -- codex
```

This creates `.stashbase/agents/codex.toml` with no egress destinations granted
by default. See the [agent-profile cookbook](docs/agent-profiles.md) for
credential-specific HTTP allow/deny rules, legacy host-only profiles, sandbox
containment, auditing, and remote sessions.

For a coding agent, use an agent profile instead of allowing the agent to select
its own secret names or destinations. Create
`.stashbase/agents/coding.toml`:

```toml
egress_hosts = ["collector.github.com"]

[secrets]
project = "my-project"
environment = "development"

[secrets.GH_TOKEN]
hosts = ["api.github.com"]
```

Profiles can instead use a fixed local secrets file. The profile owns this path;
the agent cannot provide a different file at runtime.

```toml
# .stashbase/agents/local-coding.toml
file = "/absolute/path/to/.env.agent"

[secrets.GH_TOKEN]
hosts = ["api.github.com"]
```

Then start the agent through the restricted command:

```bash
stashbase agent run --profile coding -- codex
```



Validate a profile without loading any secret before using it:

```bash
stashbase agent validate --profile coding
stashbase agent validate --remote --profile coding
```

To inspect a proposed request against a profile without loading secrets or
making a network request:

```bash
stashbase agent explain --profile coding \
  --host api.github.com --method GET --path /user
```

`agent run` always uses proxy mode, exposes only placeholders to the child,
suppresses secret printing, and strictly denies HTTP(S) destinations outside the
profile. A placeholder can only be exchanged for its mapped secret at one of
that secret's configured hosts. The agent run deliberately has no `--set`,
`--file`, `--only`, or host-override options.

Profiles can also restrict filesystem access for local agent runs:

```toml
[filesystem]
deny_read = [".env", "~/.ssh"]
deny_write = [".git", "~/.ssh"]
```

These policy-only local profiles do not require secrets or personal credentials.
Filesystem enforcement uses the supported macOS or Linux process sandbox; see
the [agent-profile cookbook](docs/agent-profiles.md) for platform requirements
and limitations.

On Linux, the CLI prefers `systemd-run --user` with `InaccessiblePaths` and
`ReadOnlyPaths`. If the systemd user session is unavailable, it probes and can
use `bubblewrap` to create a private mount namespace: read-denied directories
are overlaid with `tmpfs`, read-denied files with `/dev/null`, and write-denied
paths are remounted read-only. If neither backend can be probed, validation and
launch fail closed. Bubblewrap namespace restrictions are inherited by
descendant processes. Existing file descriptors and data already loaded into
memory are outside this policy.

Proxy mode clears inherited `NO_PROXY`, `ALL_PROXY`, and npm proxy override
variables before applying its own proxy settings, preventing common accidental
proxy bypasses. This does not stop a tool from deliberately creating a direct
connection; use `--sandbox` on supported platforms when direct network egress must be blocked.

By default, a secret is exchanged from `Authorization: Bearer <placeholder>`.
For providers with a different credential header, set `header` and optionally
`value_template` (which must contain `{value}`):

```toml
# .stashbase/agents/claude.toml
[secrets.ANTHROPIC_API_KEY]
hosts = ["api.anthropic.com"]
header = "x-api-key"
env = "ANTHROPIC_API_KEY"
# Opaque format-compatible value; never a real Anthropic key.
placeholder = "sk-ant-api03-stashbase-placeholder-000000000000000000000000000000000000"
```

Hosts may use a leading subdomain wildcard such as `*.githubcopilot.com`; it
matches subdomains only, never the apex domain itself.

When configured, `egress_hosts` controls where the agent may connect,
including ordinary traffic without a Stashbase credential. A secret's `hosts`
list is its legacy credential host allowlist; it remains active when that
secret has no `rules`. For method-and-path restrictions, add
credential-specific `rules`: rules are unordered, multiple allows are
additive, any matching deny wins, and a secret with rules is default-deny when
no allow matches. Rules never widen ordinary egress.
Use `egress_hosts = ["*"]` only when the agent needs unrestricted HTTP(S)
egress; it does not widen a secret's configured injection hosts.

HTTP MCP server profiles use `allow_tools` and `deny_tools` as authorization
terms. A denied tool is not merely labeled as denied: the Agent Proxy removes
it from `tools/list`, so the agent does not discover it, and rejects any direct
`tools/call` attempt for it. `deny_tools` takes precedence over `allow_tools`.

```toml
# .stashbase/agents/coding.toml
egress_hosts = ["api.github.com"]

[secrets.GH_TOKEN]
from = "GITHUB_TOKEN"

[[secrets.GH_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET"]
paths = ["/repos/*/*", "/repos/*/*/issues*"]

[[secrets.GH_TOKEN.rules]]
effect = "deny"
hosts = ["api.github.com"]
methods = ["DELETE"]
paths = ["*"]
```

Egress is a developer policy choice in this local mode. If a profile allows
your Stashbase API host (including through `egress_hosts = ["*"]`), a child may
run ordinary Stashbase CLI commands—including `stashbase secrets list`—with the
developer's locally stored normal authentication and retrieve authorized
secrets. Tight profiles should allow only required tool hosts; unlisted
Stashbase API hosts are denied and recorded as `host_denied` in the audit log.
Some HTTPS clients report a CONNECT-level denial as a generic connection error.
Use `--sandbox` on
supported platforms to prevent direct network bypasses. Scoped agent-session tokens will add
server-enforced permissions in a future release.

For a practical local-agent profile, allow ordinary internet access while
blocking the Stashbase API explicitly. `deny_hosts` always wins over both
`egress_hosts` and a secret's `hosts` list:

```toml
egress_hosts = ["*"]
deny_hosts = ["api.stashbase.dev"]
```

Use the hostname from `STASHBASE_API_URL` instead when targeting a custom API.

For repositories with more than one agent, keep one direct profile per file in
`.stashbase/agents/<name>.toml`. These files never store API keys or secret
values:

```toml
# .stashbase/agents/coding.toml
file = ".env.agent"
egress_hosts = ["registry.npmjs.org"]

[secrets.GH_TOKEN]
hosts = ["api.github.com"]
```

Select where the profile is loaded with `--profile-source`:

```bash
# Default: ./.stashbase/agents/coding.toml, otherwise global config
stashbase agent run --profile coding -- codex

# Require a repository-local profile
stashbase agent run --profile coding --profile-source directory -- codex

# Use a repository-local profile when present, otherwise global config
stashbase agent run --profile coding --profile-source auto -- codex
```

The default is `auto`: `.stashbase/agents/<profile>.toml` is preferred;
otherwise Stashbase falls back to global config. Treat a
repository profile as trusted policy: it can select its Stashbase environment
or local secret file and determines where secrets may be sent.
When `auto` selects a directory profile, the CLI prints a warning so the policy
choice is visible before secrets are loaded.

An egress-only profile needs neither a secret source nor a `secrets` table. It
still starts the proxy and enforces its destination policy, but grants the
child no Stashbase-managed credentials:

```toml
# .stashbase/agents/codex.toml
egress_hosts = ["chatgpt.com", "mcp.context7.com"]
deny_hosts = ["api.stashbase.dev"]
```

The CLI prints an explicit warning when this mode starts.

An agent profile may define both a Stashbase `[secrets]` project/environment source and a
local `file`. The file is a local override: its configured source names win,
and Stashbase requests only the remaining profile sources from the API.

```toml
# .stashbase/agents/coding.toml
file = ".env.local"

[secrets]
project = "platform"
environment = "development"

[secrets.GH_TOKEN]
from = "GITHUB_TOKEN"
hosts = ["api.github.com"]
```

Here `.env.local` may provide `GITHUB_TOKEN`; otherwise the CLI fetches that
source from the configured Stashbase environment.

File-only agent profiles do not require a Stashbase API key. A key is required
only when the run needs one or more remote project/environment sources.

See the [agent proxy profile cookbook](docs/agent-profiles.md) for ready-made
GitHub Copilot and OpenAI API client profiles, plus guidance for unsupported
header formats.

Some tools, including some `gh` builds, ignore the CA-file environment variables
used by the proxy. Opt into temporary operating-system trust-store integration
for those tools:

```bash
stashbase agent run --profile coding --trust-proxy-ca -- codex
```

The temporary CA is removed when the command finishes. On macOS this uses the
login Keychain; on Windows it uses the current-user Root store; on Linux it uses
the platform's system trust-store updater and may prompt for `sudo`. This option
intentionally changes host trust only for the session and should be used only on
a machine where the launched agent is trusted.

### Network sandbox (experimental)

On macOS and systemd-based Linux systems, add `--sandbox` to deny the child
direct network access while retaining its loopback connection to the embedded
proxy:

```bash
stashbase agent run --sandbox --profile coding --profile-source directory -- codex
```

This prevents a sandboxed tool from bypassing the proxy with a direct internet
connection. macOS uses the deprecated `sandbox-exec` utility. Linux uses
`systemd-run --user --scope` with cgroup IP allow/deny rules, so it requires
`systemd-run` and an active systemd user session. Windows is not implemented.
This is network containment only, not filesystem or same-user process-memory
isolation.

#### Remote Agent Proxy sessions

For a Stashbase-backed profile with `[secrets]` `project` and `environment`, add
`--remote` to keep resolved credentials in the control plane:

```bash
stashbase agent run --remote --profile coding -- codex
```

The child receives only configured opaque placeholders and uses a temporary
localhost relay through `HTTP_PROXY` and `HTTPS_PROXY`. The session token and
resolved secret values remain out of the child environment, and the short-lived
session is ended when the child exits. Remote sessions do not support local-file
or egress-only profiles.

### Threat model and security boundary

`agent run` is designed to reduce accidental or normal agent-tool exposure of
credentials during local development. The child receives placeholders rather
than real secret values; the proxy replaces those placeholders only in the
configured request header, only for that secret's approved hosts. Strict egress
policy and audit logs make those proxied HTTP(S) decisions visible.

It is not a security boundary against a malicious or compromised process
running as the same user. Such a process may inspect local files or process
memory, alter the environment, invoke ordinary `stashbase run`, or otherwise
bypass the intended workflow. Without `--sandbox`, a tool that ignores
proxy environment variables can also make direct network connections. The
sandbox reduces that bypass route, but does not provide filesystem,
process-memory, kernel, administrator, or root isolation.

As defense in depth, `agent run` removes the inherited `STASHBASE_API_KEY`
environment variable from the child. This does not prevent a same-user process
from accessing credentials stored elsewhere, such as CLI configuration or the
operating-system credential store.

Treat directory profiles as trusted policy: with the default `--profile-source
auto`, a repository `.stashbase/agents/<profile>.toml` file can select a secret
source and its allowed destinations. Do not run an agent with secrets from an
untrusted repository, or give it unrestricted Stashbase API credentials.

### Audit logs

`agent run` writes a private JSONL audit log by default. It records session
events and proxy decisions (destination host, method, secret name, status, and
duration), never secret values, placeholders, headers, bodies, URLs, or command
arguments. Logs are stored per session under the Stashbase config directory and
are permission-restricted on Unix. On each agent run, logs older than 30 days
are removed and storage is capped at 1,000 session files. Disable persistence
for a session with:

```bash
stashbase agent run --audit-log false --profile coding -- codex
```

Failure actions include `host_denied`, `unknown_placeholder`,
`tls_trust_failed`, `upstream_timeout`, and `upstream_connection_failed`.
An unknown or stale placeholder is denied before forwarding. A direct proxy
bypass cannot be logged because the request never reaches the proxy; use the
`--sandbox` option on supported platforms when that containment matters.

View the recent local proxy decisions without reading JSONL files directly:

```bash
stashbase agent logs
stashbase agent logs --since 24h --limit 100
stashbase agent logs --profile coding --action injected --host api.github.com
stashbase agent logs --session <session-id>
stashbase agent logs --follow
```

`--json` returns a JSON array for a one-time view. With `--follow`, it emits
one JSON event per line as new events arrive. Profile, action, host, and session
filters use exact matches. Each audited `agent run` prints its session ID at
startup, which can be passed to `--session`.

This is still a local experimental mode. If a profile permits the Stashbase API
host, a sandboxed agent can still invoke normal `stashbase` commands through the
proxy using same-user credentials. Use `deny_hosts` for the Stashbase API host
when that route must be blocked.

### Generate utility values

```bash
# generate random uuid v4
stashbase generate uuid v4

# generate random hex string
stashbase generate random hex --bytes 16 --uppercase

# generate random base64 string
stashbase generate random base64 --length 32 --uppercase

# generate SHA-256 hash from value
stashbase generate hash "my-secret-value"

# generate SHA-512 hash from value
stashbase generate hash "my-secret-value" --algorithm sha512

# generate random passphrase
stashbase generate passphrase --words 6 --separator "-"

# generate SSH key pair
stashbase generate ssh-keypair --out ~/.ssh/id_stashbase --comment "you@company.com"
```

### Scan for hardcoded secrets

```bash
## scan staged files to be committed
stashbase scan staged

## scan all changed files (staged and unstaged)
stashbase scan changes

## scan commits to be pushed to remote
stashbase scan unpushed

## install scan hook into Husky pre-commit file
stashbase scan install pre-commit --file .husky/pre-commit

## install both pre-commit and pre-push hooks
stashbase scan install --all

## uninstall scan hook from Husky pre-commit file
stashbase scan uninstall pre-commit --file .husky/pre-commit
```

### Diagnose CLI setup

```bash
# run local diagnostics
stashbase doctor

# include live API auth check
stashbase doctor --auth-check

# show detailed diagnostics
stashbase doctor --verbose
```

## Contributing

Bug fixes, documentation improvements, and improvements of all kinds are always welcome.

See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.


## License

Stashbase CLI is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
You can find the license in the [LICENSE.txt](LICENSE.txt) file.

## Contact

If you have any questions or feedback, please contact us at [support@stashbase.dev](mailto:support@stashbase.dev).
