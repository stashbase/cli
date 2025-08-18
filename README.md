# Stashbase CLI

The Stashbase CLI is official command line tool for Stashbase secrets management platform for developers.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [License](#license)
- [Contact](#contact)

## Installation

You have multiple options to install the Stashbase CLI, either via package managers, shell script ot you can just download the binary from the [releases](https://github.com/stashbase/cli/releases) page directly.

### macOS

Stashbase CLI is available via [Homebrew](https://brew.sh/) for macOS users.

```bash
brew install stashbase/stashbase-cli
```

### Linux

For Linux users, we recommend dowload Stashbase CLI using shell script.

```bash
curl -fsSL https://stashbase.com/cli/install.sh | bash
```

### Windows

For Windows users, we recommend dowload Stashbase CLI using shell script or package manager [Scoop](https://scoop.sh).

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

You can generate API key in your Stashbase workspace going to API Keys -> Personal API Keys -> Create API Key.

```bash
stashbase config set api-key <API_KEY>
```

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

### Generate random string (utility)

```bash
# generate random uuid v4
stashbase generate uuid v4

# generate random hex string
stashbase generate random hex --bytes 16 --uppercase

# generate random base64 string
stashbase generate random base64 --length 32 --uppercase
```

### Scan for hardcoded secrets

```bash
## scan staged files to be committed
stashbase scan staged

## scan commits to be pushed to remote
stashbase scan commits
```

## License

Stashbase CLI is licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
You can find the license in the [LICENSE](LICENSE) file.

## Contact

If you have any questions or feedback, please contact us at [support@stashbase.com](mailto:support@stashbase.com).
