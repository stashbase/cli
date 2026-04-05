# Stashbase CLI (Beta)

The Stashbase CLI is the official command-line tool for the Stashbase secrets management platform for developers.

⚠️ **Beta:** This CLI is in beta. Features and commands may change.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [License](#license)
- [Contact](#contact)

## Installation

You have multiple options to install the Stashbase CLI, either via package managers, shell script or you can just download the binary from the [releases](https://github.com/stashbase/cli/releases) page directly.

### macOS

Stashbase CLI is available via [Homebrew](https://brew.sh/) for macOS users.

```bash
brew install stashbase/stashbase-cli
```

### Linux

For Linux users, we recommend downloading Stashbase CLI using shell script.

```bash
curl -fsSL https://stashbase.com/cli/install.sh | bash
```

### Windows

For Windows users, we recommend downloading Stashbase CLI using shell script or package manager [Scoop](https://scoop.sh).

```bash
curl -fsSL https://stashbase.com/cli/install.sh | bash
```

```bash
scoop install stashbase-cli
```

## Usage

For full documentation, please visit [Stashbase CLI Documentation](https://docs.stashbase.com/cli).

Here are some common usage examples for the Stashbase CLI:

### Authenticate with Stashbase

You can generate an API key in your Stashbase workspace by going to API Keys -> Personal API Keys -> Create API Key.

```bash
stashbase config set api-key <API_KEY>
```

### API key storage

`stashbase config set api-key` stores your API key in the OS secure credential store:
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
stashbase environments list -p <PROJECT_ID_OR_NAME>
```

### List secrets

```bash
stashbase secrets list -p <PROJECT_ID_OR_NAME> -e <ENVIRONMENT_ID_OR_NAME>
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
stashbase scan commits
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

## License

Stashbase CLI is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
You can find the license in the [LICENSE.txt](LICENSE.txt) file.

## Contact

If you have any questions or feedback, please contact us at [support@stashbase.com](mailto:support@stashbase.com).
