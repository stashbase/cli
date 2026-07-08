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

Here are some common usage examples for the Stashbase CLI:

### Authenticate with Stashbase

You can generate an API key in your Stashbase workspace by going to API Keys -> Personal API Keys -> Create API Key.

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
