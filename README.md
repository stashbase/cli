# Stashbase CLI

Run Claude Code, Codex, Cursor, and other coding agents through a local or remote Agent Proxy with controlled access to secrets, APIs, files, MCP tools, and dependencies—without exposing raw credentials.

Stashbase is an open-source access layer that gives coding agents the access they need while keeping raw credentials out of the agent process. Instead of handing out API keys, agents receive placeholders that the Agent Proxy exchanges only for approved destinations. Audit logs record proxy-mediated requests and policy decisions.

## Table of Contents

- [Agent Quickstart](#agent-quickstart)
- [What Stashbase Controls](#what-stashbase-controls)
- [Agent Proxy and Profiles](#agent-proxy-and-profiles)
  - [Basic Example](#basic-example)
  - [How the Agent Proxy Works](#how-the-agent-proxy-works)
  - [Profile Syntax and Configuration](#profile-syntax-and-configuration)
  - [Filesystem and Network Containment](#filesystem-and-network-containment)
  - [Docker Sandbox Backend (Experimental)](#docker-sandbox-backend-experimental)
  - [Remote Agent Sessions](#remote-agent-sessions)
  - [MCP Tools Authorization](#mcp-tools-authorization)
  - [Audit Logs and Session Revocation](#audit-logs-and-session-revocation)
- [Threat Model and Security Boundary](#threat-model-and-security-boundary)
- [Secret Management and Developer Workflows](#secret-management-and-developer-workflows)
  - [CLI Profiles](#cli-profiles)
  - [First-time Setup](#first-time-setup)
  - [Authentication](#authentication)
  - [List and Export Secrets](#list-and-export-secrets)
  - [Run Commands with Injected Secrets](#run-commands-with-injected-secrets)
  - [Generate Utility Values](#generate-utility-values)
  - [Scan for Hardcoded Secrets](#scan-for-hardcoded-secrets)
  - [Dependency Security Hooks](#dependency-security-hooks)
  - [Diagnose CLI Setup](#diagnose-cli-setup)
- [Installation](#installation)
- [Contributing, License, and Contact](#contributing-license-and-contact)

## Agent Quickstart

Start here if you want to run a coding agent with controlled access to your secrets.

1. **Install Stashbase CLI** (see [Installation](#installation) for your platform)

2. **Initialize your setup and create an agent profile:**

```bash
# Initial setup (creates credentials)
stashbase setup

# Create an agent profile (generates .stashbase/agents/coding.toml)
stashbase agent init coding

# Validate the profile before using it
stashbase agent validate --profile coding

# Run your agent with the profile
stashbase agent run --profile coding -- codex
```

Replace `codex` with `claude` for Claude Code, or `cursor` for Cursor.

A fresh profile grants no egress destinations. Edit `.stashbase/agents/coding.toml` to add the secrets and API hosts your agent needs. See [Basic Example](#basic-example) for a complete, working profile.

## What Stashbase Controls

Agent profiles let you control:

- **Secrets and personal credentials without raw values**: Agents receive placeholders instead of real credentials. Application secrets are team-shared credentials stored in a project/environment (e.g., shared API keys). Personal credentials are your own account-scoped credentials, accessible across workspaces and available only in remote sessions. The proxy exchanges placeholders only in configured HTTP(S) request headers, only to approved hosts.
- **API hosts, methods, and paths**: Allow or deny specific destinations—for example, `GET /repos/*/*` on `api.github.com` but deny `DELETE /*`.
- **Filesystem access**: Block reads from `.env`, `~/.ssh`, or other sensitive directories. Block writes to `.git` or shared config.
- **Network egress**: Agents connect only through a loopback proxy. Direct connections to unapproved hosts are denied.
- **MCP tools**: Authorize or deny which MCP tools the agent can discover and call.
- **Dependency installation hooks**: Scan and block suspicious package installs before they run.
- **Local and remote Agent Proxy sessions**: Run agents locally through a policy-controlled proxy, or use remote sessions where credentials remain in Stashbase and never reach your machine. Manage, monitor, and revoke remote sessions from Stashbase.
- **Audit logs and revocation**: Proxy decisions are logged by default. Revoke individual local or remote sessions and inspect their proxy activity.

## Agent Proxy and Profiles

### Basic Example

Create `.stashbase/agents/coding.toml`:

```toml
egress_hosts = ["api.github.com", "registry.npmjs.org"]

[secrets]
project = "my-project"
environment = "development"

[secrets.GH_TOKEN]
[[secrets.GH_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET", "POST"]
paths = ["/*"]

[filesystem]
deny_read = [".env", "~/.ssh"]
deny_write = [".git"]
```

Now your agent can:
- Use the `GH_TOKEN` secret on GitHub API requests (it receives a placeholder, not the real token).
- Reach the npm registry when allowed by the profile. Enable the dependency hook separately if you want package-install checks.
- Read files anywhere except `.env` and `~/.ssh`.
- Write files anywhere except `.git`.

### How the Agent Proxy Works

When you run `stashbase agent run --profile coding -- codex`, Stashbase:

1. Loads your profile and the secrets it references.
2. Starts a temporary HTTP proxy on localhost.
3. Passes placeholders to Codex (e.g., `**STASHBASE_GH_TOKEN**`) instead of real credentials.
4. When Codex sends a request with that placeholder in an `Authorization: Bearer` header, the proxy replaces it with the actual token **only if** the destination is in the profile's allow list.
5. Requests to unapproved hosts are denied. Audit logs record every decision.
6. The proxy stops and cleans up when Codex exits.

This design keeps raw secrets out of the agent's process memory and environment variables. The proxy itself necessarily handles the real secret in memory. This makes it less likely the agent accidentally exposes credentials through its environment or logs, which contain only placeholders.

### Profile Syntax and Configuration

A profile is a TOML file at `.stashbase/agents/<name>.toml`. Use `stashbase agent init <name>` to generate a starter.

#### Egress and credential hosts

```toml
# All HTTP(S) destinations the agent is allowed to reach
egress_hosts = ["api.github.com", "registry.npmjs.org"]

# (Optional) Block specific destinations even if egress_hosts is wide
deny_hosts = ["api.stashbase.dev"]

[secrets]
project = "my-project"
environment = "development"

# Each secret maps to a name in your Stashbase environment
[secrets.GH_TOKEN]
[[secrets.GH_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET", "POST"]
paths = ["/*"]
```

#### Credential injection headers

By default, placeholders are exchanged in `Authorization: Bearer <placeholder>` headers. For other header formats:

```toml
[secrets.ANTHROPIC_API_KEY]
header = "x-api-key"
placeholder = "sk-ant-stashbase-placeholder-000000000000000000000000"
[[secrets.ANTHROPIC_API_KEY.rules]]
effect = "allow"
hosts = ["api.anthropic.com"]
methods = ["POST"]
paths = ["/v1/*"]
```

#### Method and path restrictions

For granular control, use `rules`:

```toml
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

Rules are unordered; any matching deny wins, and a secret with rules is default-deny when no allow matches.

#### Local secret files

Instead of fetching secrets from Stashbase, use a local file:

```toml
file = "/absolute/path/to/.env.agent"

[secrets.GH_TOKEN]
[[secrets.GH_TOKEN.rules]]
effect = "allow"
hosts = ["api.github.com"]
methods = ["GET", "POST"]
paths = ["/*"]
```

#### Personal credentials (remote sessions only)

Personal credentials are your own user-specific credentials stored in your Stashbase account (as opposed to team-shared application secrets). They're accessible across workspaces and available only in remote sessions. Use `[personal_credentials.NAME]` with the same rules syntax as `[secrets.NAME]`. Personal credentials require `--remote` and are never stored or exported by the CLI:

```toml
[personal_credentials.LINEAR_API_KEY]
env = "LINEAR_API_KEY"

[[personal_credentials.LINEAR_API_KEY.rules]]
effect = "allow"
hosts = ["mcp.linear.app"]
methods = ["GET", "POST"]
paths = ["/mcp"]
```

Personal credentials support the same fields as application secrets: `env`, `from`, `header`, `value_template`, and `rules`. The agent receives only a placeholder, never the raw personal credential value.

#### Egress-only profiles (no secrets)

If the agent needs no credentials, only egress policy:

```toml
egress_hosts = ["chatgpt.com", "mcp.context7.com"]
deny_hosts = ["api.stashbase.dev"]
```

Stashbase will warn when starting this mode.

### Filesystem and Network Containment

#### Read and write restrictions

Profiles can deny access to sensitive paths:

```toml
[filesystem]
deny_read = [".env", "~/.ssh", "~/.aws"]
deny_write = [".git", "~/.ssh", "~/.aws"]
```

On macOS, Stashbase wraps the agent in Seatbelt, which enforces filesystem rules. On Linux and WSL2, it uses `systemd-run --user` with cgroup IP rules, or falls back to `bubblewrap` for namespace isolation. Windows native is not implemented; use WSL2 instead.

Denied reads return `/dev/null`; denied writes go to an empty overlay. Existing file descriptors and data already in memory are not affected. These are policy-only; these profiles do not require secrets.

#### Network containment

Every `agent run` denies the child direct network access. The agent communicates only through the embedded proxy on localhost. This prevents a tool from bypassing the proxy with a direct internet connection.

On macOS, this uses the deprecated `sandbox-exec` utility. On Linux and WSL2, it uses `systemd-run --user --scope` with cgroup rules.

This is network containment only, not filesystem, process-memory, or kernel isolation.

**If Docker is available, prefer the Docker sandbox backend below over the native one** — it's meaningfully stronger: filesystem access is allow-list rather than deny-list (nothing outside the working directory is visible at all, instead of specific paths being blocked), network egress is enforced at the network layer rather than relying on the agent to honor its proxy environment variables, and it works identically across macOS, Linux, and Windows (via Docker Desktop) instead of needing platform-specific mechanisms with a Windows gap. The native backend remains the default for now since it needs nothing beyond the CLI itself, but Docker is the recommended choice whenever it's an option.

### Docker Sandbox Backend (Experimental)

The recommended backend when Docker is available: the agent runs inside a Docker container instead of a same-host sandboxed process, with allow-list filesystem access and a network-layer firewall (enforced even against an agent that deliberately ignores its proxy env vars).

```toml
[sandbox]
backend = "docker"
```

```bash
stashbase agent run --profile coding -- claude
```

Or override the profile's choice for one invocation without editing the file: `--docker-sandbox true|false`, per-run image overrides with `--docker-image <ref>` / `--docker-dockerfile <path>`, and resource caps with `--docker-memory <value>` / `--docker-cpus <value>` (also settable per profile via `[sandbox] memory`/`cpus`; no cap by default).

Claude Code and Codex are pre-installed in the default sandbox image; a profile can also run its own image or Dockerfile instead (`[sandbox] image`/`dockerfile`) to add other tools, without loosening any of the sandbox constraints themselves.

See **[docs/sandboxing.md](docs/sandboxing.md)** for the full picture: how the network firewall is enforced, custom images, git identity forwarding, login persistence across images, Codex/Claude Code OAuth quirks, and current limitations.

### Remote Agent Sessions

Use `--remote` to run with credentials managed entirely in the Stashbase control plane. Profiles can use either application secrets with `[secrets]` (requires `project` and `environment`) or user-specific `[personal_credentials]` (no Stashbase API key required):

```bash
stashbase agent run --remote --profile coding -- codex
```

The child receives only placeholders and connects through a temporary localhost relay. Session tokens and resolved credential values stay out of the child environment and never reach your machine. Personal credentials remain private to your account. The session is managed from Stashbase, where you can monitor and revoke it remotely before the child exits.

Remote Agent Proxy is not a general network sandbox; it relays supported HTTP traffic for supported coding-agent workflows. SSH, raw TCP, and arbitrary third-party integrations are unsupported.

### MCP Tools Authorization

HTTP MCP server profiles can control which tools are visible to the agent. Define a secret for authentication and an MCP server entry with tool authorization:

```toml
[secrets.MCP_TOKEN]
[[secrets.MCP_TOKEN.rules]]
effect = "allow"
hosts = ["mcp.example.com"]
methods = ["GET", "POST"]
paths = ["/mcp"]

[mcp_servers.example]
url = "https://mcp.example.com/mcp"
binding = "MCP_TOKEN"
allow_tools = ["search_issues", "read_file"]
deny_tools = ["delete_repo"]
```

Denied tools are removed from `tools/list`, so the agent does not discover them. Direct `tools/call` attempts for denied tools are rejected. `deny_tools` takes precedence. An omitted or empty `allow_tools` allows no tools; use `allow_tools = ["*"]` to allow every tool.

For configuration examples and the `agent mcp tools`, `check`, and `verify` commands, see the [agent-profile cookbook](docs/agent-profiles.md#http-mcp-servers).

### Audit Logs and Session Revocation

Every `agent run` writes a private JSONL audit log by default. It records session events and proxy decisions (destination host, method, secret name, status, and duration), never secret values, placeholders, headers, bodies, or command arguments. Logs are stored per session under the Stashbase config directory and are permission-restricted on Unix. Logs older than 30 days are cleaned up automatically; storage is capped at 1,000 session files. Disable persistence for a session with:

```bash
stashbase agent run --audit-log false --profile coding -- codex
```

View recent proxy decisions:

```bash
stashbase agent logs
stashbase agent logs --since 24h --limit 100
stashbase agent logs --profile coding --action injected --host api.github.com
stashbase agent logs --session <session-id>
stashbase agent logs --follow
```

Use `--json` for a JSON array; with `--follow`, events stream as one JSON object per line.

List and revoke active sessions:

```bash
stashbase agent sessions list
stashbase agent sessions list --local
stashbase agent sessions list --remote
stashbase agent sessions revoke <session-id>
stashbase agent sessions revoke --all --local
stashbase agent sessions revoke --all --remote
```

Local revocation stops the local proxy process. Remote revocation ends the logical session, including rotated tokens. Bulk revocation requires either `--local` or `--remote` and prompts for confirmation; use `--silent` to skip it.

## Threat Model and Security Boundary

`agent run` is designed to reduce accidental or normal agent-tool exposure of credentials during local development. The child receives placeholders rather than real secret values; the proxy replaces those placeholders only in configured HTTP(S) request headers and only for that secret's approved hosts. Strict egress policy and audit logs make those proxied decisions visible.

**It is not a security boundary against a malicious or compromised process running as the same user.** Such a process may inspect local files or process memory, alter the environment, invoke ordinary `stashbase run`, or otherwise bypass the intended workflow. The network sandbox blocks direct connections from the agent child, but does not provide filesystem, process-memory, kernel, administrator, or root isolation.

As defense in depth, `agent run` removes the inherited `STASHBASE_API_KEY` environment variable from the child. This does not prevent a same-user process from accessing credentials stored elsewhere, such as CLI configuration or the operating-system credential store.

**Treat directory profiles as trusted policy.** With the default `--profile-source auto`, a repository `.stashbase/agents/<profile>.toml` can select a secret source and its allowed destinations. Do not run an agent with secrets from an untrusted repository, or give it unrestricted Stashbase API credentials.

## Secret Management and Developer Workflows

The CLI also supports traditional secret management for developers and CI/CD.

### CLI Profiles

Use profiles when you work with more than one Stashbase workspace. Each profile's API key is stored separately in the OS secure credential store; the config file contains only the profile name and optional workspace label.

```bash
# Create a profile and enter its API key when prompted.
stashbase config profile add acme --workspace acme-production

# Make it your usual profile.
stashbase config profile use acme

# Use a profile for one command or an automation run.
STASHBASE_PROFILE=acme stashbase projects list
```

Selection uses `STASHBASE_PROFILE`, then the configured default profile (or `default`). `--api-key` and `STASHBASE_API_KEY` override the selected profile's stored key, which is useful in CI.

### First-time Setup

After installing Stashbase CLI, run:

```bash
stashbase setup
```

This creates your initial profile (defaults to `default`) and prompts for your Stashbase API key. If you skip the API key, set it later with `stashbase config api-key set`.

For full documentation, visit [Stashbase CLI Documentation](https://docs.stashbase.dev/cli).

### Authentication

Generate an API Key in your Stashbase workspace at **API Keys → Personal API Keys → Create API Key**.

```bash
# Interactively set your API key
stashbase config api-key set

# Or set it via stdin (from environment variable or another secret manager)
printf '%s' 'sb_personal_35tnv...' | stashbase config api-key set --stdin
```

API keys are stored in your OS secure credential store:
- macOS: Keychain
- Linux: Secret Service (`secret-tool`)
- Windows: DPAPI-encrypted local secret file

If secure storage is unavailable, the CLI falls back to config-file storage and prints a warning. The config file is written with owner-only permissions on Unix systems and contains no secret values.

### List and Export Secrets

```bash
# List all projects
stashbase projects list

# List environments in a project
stashbase environments list -p <PROJECT>

# List secrets in a project/environment
stashbase secrets list -p <PROJECT> -e <ENVIRONMENT>

# Export an environment schema (names and metadata, no values)
stashbase secrets schema pull --project <PROJECT> --environment <ENVIRONMENT>
```

The schema export writes `env.schema.yaml` with project/environment metadata and secret names, never values or IDs. Use `--output <PATH>` for a different location.

### Run Commands with Injected Secrets

For one-off commands or CI/CD, use `stashbase run` to load secrets and execute a command:

```bash
# Load from interactive selection
stashbase run -- npm run dev

# Load from a project and environment
stashbase run -p <PROJECT> -e <ENVIRONMENT> -- npm run dev

# Load from a local dotenv, YAML, or JSON file
stashbase run --file .env.production -- npm run dev
stashbase run --file secrets.yaml -- npm run dev
```

For proxy mode on `stashbase run` (not `agent run`), use `--proxy` with optional `--proxy-port`:

```bash
stashbase run --proxy --only GH_TOKEN -- gh workflow run deploy.yml
stashbase run --proxy --proxy-port 8787 --only GH_TOKEN -- gh auth status
```

This is the same proxy mechanism as `agent run`, but designed for individual commands rather than long-lived agents. It is a feasibility experiment, not a production isolation boundary. HTTPS rewriting uses TLS interception, so the proxy creates a temporary local CA and passes its path via `SSL_CERT_FILE`, `CURL_CA_BUNDLE`, and `GIT_SSL_CAINFO`. Tools that ignore these variables, pin certificates, use HTTP/2-only traffic, or bypass proxy environment variables will not work.

### Generate Utility Values

```bash
# Generate random UUIDs and values
stashbase generate uuid v4
stashbase generate random hex --bytes 16 --uppercase
stashbase generate random base64 --length 32 --uppercase

# Generate hashes
stashbase generate hash "my-secret-value"
stashbase generate hash "my-secret-value" --algorithm sha512

# Generate passphrases and SSH key pairs
stashbase generate passphrase --words 6 --separator "-"
stashbase generate ssh-keypair --out ~/.ssh/id_stashbase --comment "you@company.com"
```

### Scan for Hardcoded Secrets

Detect accidental credential commits before they reach version control:

```bash
# Scan staged files
stashbase scan staged

# Scan all changed files (staged and unstaged)
stashbase scan changes

# Scan commits ready to push
stashbase scan unpushed

# Install hook into Husky pre-commit
stashbase scan install pre-commit --file .husky/pre-commit

# Install both pre-commit and pre-push hooks
stashbase scan install --all

# Remove hook from Husky
stashbase scan uninstall pre-commit --file .husky/pre-commit
```

### Dependency Security Hooks

Block suspicious package installs before they run. Install hooks for Codex, Claude Code, or Cursor:

```bash
# Install in the current repository
stashbase agent hooks deps install codex
stashbase agent hooks deps install claude
stashbase agent hooks deps install cursor

# Install globally (all repositories)
stashbase agent hooks deps install codex --global
stashbase agent hooks deps install claude --global
stashbase agent hooks deps install cursor --global

# Check hook status (non-zero exit if missing)
stashbase agent hooks deps check claude
stashbase agent hooks deps check claude --global

# Remove hooks from current repository
stashbase agent hooks deps uninstall codex
stashbase agent hooks deps uninstall claude
stashbase agent hooks deps uninstall cursor

# Remove global hooks
stashbase agent hooks deps uninstall codex --global
stashbase agent hooks deps uninstall claude --global
stashbase agent hooks deps uninstall cursor --global
```

Use `dependencies` as an alias for `deps`. The hook supports npm, Bun, pnpm, and Yarn. For package-specific installs, it sends only package names and versions to Stashbase. For project-wide installs like `npm ci`, it scans direct dependencies from `package.json` and uses versions from lockfiles when available. This is not a full dependency-tree audit; transitive dependencies are not scanned.

### Diagnose CLI Setup

```bash
# Run local diagnostics
stashbase doctor

# Include API authentication check
stashbase doctor --auth-check

# Show detailed output
stashbase doctor --verbose
```

## Installation

Stashbase CLI is available for macOS Apple Silicon, Linux x64, and Windows x64. Intel macOS is not supported. Choose your platform:

### macOS

Use [Homebrew](https://brew.sh) for Apple Silicon:

```bash
brew tap stashbase/homebrew-stashbase
brew trust stashbase/stashbase
brew install stashbase
```

Or download directly from [releases](https://github.com/stashbase/cli/releases).

### Linux

Use the installation script:

```bash
curl -fsSL https://stashbase.dev/cli/install.sh | bash
```

Or download from [releases](https://github.com/stashbase/cli/releases).

### Windows

For general commands, use the native Windows CLI via [Scoop](https://scoop.sh):

```bash
scoop bucket add stashbase https://github.com/stashbase/scoop-stashbase
scoop install stashbase
```

**For `agent run`**, use the Linux CLI in WSL2 (Stashbase needs Linux process containment). Enable systemd and disable Windows interop in `/etc/wsl.conf`:

```ini
[boot]
systemd=true

[interop]
enabled=false
appendWindowsPath=false
```

Then run `wsl --shutdown` from Windows, and install/run Stashbase inside WSL2. Keep agent workspaces in the WSL filesystem (not under `/mnt/c`).

### Initial Setup

After installing, run:

```bash
stashbase setup
```

This prompts for a profile name (defaults to `default`) and your Stashbase API key. If you skip the API key, you can set it later with `stashbase config api-key set`.

For full documentation, visit [Stashbase CLI Documentation](https://docs.stashbase.dev/cli).

## Contributing, License, and Contact

### Contributing

Bug fixes, documentation improvements, and improvements of all kinds are always welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.

### License

Stashbase CLI is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0). See [LICENSE.txt](LICENSE.txt) for details.

### Contact

For questions or feedback, contact us at [support@stashbase.dev](mailto:support@stashbase.dev).
