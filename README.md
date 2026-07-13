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

### Authenticate with Stashbase
If you don't set the API Key during setup, you can set it later manually.
You can generate an API Key in your Stashbase workspace by going to API Keys -> Personal API Keys -> Create API Key.

```bash
# interactively set API key
stashbase config api-key set

# or set API key via stdin (e.g. from environment variable or secret manager)
printf '%s' 'sb_personal_35tnv...' | stashbase config api-key set --stdin
```

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

### Experimental credential broker

`run --broker` starts an in-process, localhost-only HTTP proxy for the lifetime
of the child command. Instead of receiving the loaded secret, the child receives
a placeholder such as `**STASHBASE_GH_TOKEN**`. When the child sends that value
as an `Authorization: Bearer` header, the broker replaces it before forwarding
the request.

```bash
stashbase run --broker --only GH_TOKEN -- gh workflow run deploy.yml
```

The broker prints its temporary localhost port when it starts and stops as soon
as the child command finishes. It is not a daemon and does not write credentials
to stdout or logs.

This is a feasibility experiment, not a production credential boundary. HTTPS
rewriting requires TLS interception, so the broker creates a temporary local CA
and provides its path through standard child-process trust variables
(`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, and `GIT_SSL_CAINFO`). `curl` can use this
on typical systems. A client that ignores these variables, pins certificates,
uses HTTP/2-only proxy traffic, or bypasses proxy environment variables will
not work; in particular, `gh` may not trust the temporary CA on every platform.
Only exact `Authorization: Bearer <placeholder>` headers are rewritten. Other
credential formats, non-HTTP traffic, and approval flows are out of scope.

Node's built-in `fetch` is configured through `NODE_USE_ENV_PROXY=1` and
`NODE_EXTRA_CA_CERTS`, which the broker supplies automatically.

For safe troubleshooting, set `RUST_LOG=debug`. Broker diagnostics identify
only the denied or unreachable destination host; they never include headers or
secret values.

### Agent profiles (experimental)

For a coding agent, use an agent profile instead of allowing the agent to select
its own secret names or destinations. Add the profile to the user-level
Stashbase `config.toml`:

```toml
[agent_profiles.coding]
project = "my-project"
environment = "development"
egress_hosts = ["collector.github.com"]

[agent_profiles.coding.secrets.GH_TOKEN]
hosts = ["api.github.com"]
```

Profiles can instead use a fixed local secrets file. The profile owns this path;
the agent cannot provide a different file at runtime.

```toml
[agent_profiles.local-coding]
file = "/absolute/path/to/.env.agent"

[agent_profiles.local-coding.secrets.GH_TOKEN]
hosts = ["api.github.com"]
```

Then start the agent through the restricted command:

```bash
stashbase agent run --profile coding -- codex
```

`agent run` always uses broker mode, exposes only placeholders to the child,
suppresses secret printing, and strictly denies HTTP(S) destinations outside the
profile. A placeholder can only be exchanged for its mapped secret at one of
that secret's configured hosts. The agent command deliberately has no `--set`,
`--file`, `--only`, or host-override options.

Hosts may use a leading subdomain wildcard such as `*.githubcopilot.com`; it
matches subdomains only, never the apex domain itself.

`egress_hosts` permits ordinary traffic without injecting a Stashbase
credential. Keep a secret's `hosts` list limited to destinations that should
receive that specific credential.
Use `egress_hosts = ["*"]` only when the agent needs unrestricted HTTP(S)
egress; it does not widen a secret's configured injection hosts.

Agent profiles can also live in `.stashbase.toml` in the command's current
directory. The directory file contains a complete profile and never stores API
keys or secret values:

```toml
# .stashbase.toml
[agent_profiles.coding]
file = ".env.agent"
egress_hosts = ["registry.npmjs.org"]

[agent_profiles.coding.secrets.GH_TOKEN]
hosts = ["api.github.com"]
```

Select where the profile is loaded with `--profile-source`:

```bash
# Default: user-level Stashbase config
stashbase agent run --profile coding --profile-source global -- codex

# Require ./.stashbase.toml
stashbase agent run --profile coding --profile-source directory -- codex

# Use ./.stashbase.toml when present, otherwise global config
stashbase agent run --profile coding --profile-source auto -- codex
```

The default is `global` so simply entering a directory cannot change an
agent's credential policy. Use `directory` only for repositories you trust:
the file is security policy and can select its own Stashbase environment or
local secret file.

Some tools, including some `gh` builds, ignore the CA-file environment variables
used by the broker. Opt into temporary operating-system trust-store integration
for those tools:

```bash
stashbase agent run --profile coding --trust-broker-ca -- codex
```

The temporary CA is removed when the command finishes. On macOS this uses the
login Keychain; on Windows it uses the current-user Root store; on Linux it uses
the platform's system trust-store updater and may prompt for `sudo`. This option
intentionally changes host trust only for the session and should be used only on
a machine where the launched agent is trusted.

This is still a local experimental mode. A sandboxed agent must not have access
to the user's unrestricted Stashbase API credentials or it could invoke normal
`stashbase run` directly instead of this restricted command.

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
