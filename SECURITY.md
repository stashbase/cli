# Security Policy

Stashbase CLI keeps credentials away from coding agents, so we treat security reports as a priority.

## Reporting a vulnerability

Please do not open a public issue for security problems.

- **Preferred:** [report it privately on GitHub](https://github.com/stashbase/cli/security/advisories/new).
- **Or email** [support@stashbase.dev](mailto:support@stashbase.dev) with "Security" in the subject.

Include the CLI version (`stashbase --version`), your platform and sandbox backend, the profile involved (with secret values removed), and steps to reproduce. A proof of concept helps us confirm the issue quickly.

We will acknowledge your report, keep you updated while we work on a fix, and credit you in the release notes unless you prefer otherwise.

## Supported versions

Security fixes go into the latest release. Please check that the issue still reproduces on the [latest version](https://github.com/stashbase/cli/releases/latest) before reporting.

## Scope

In scope, for example:

- An agent obtaining a raw secret value instead of a placeholder
- The Agent Proxy exchanging a placeholder for a destination, method, or path the profile does not allow
- Bypassing network egress rules, filesystem deny rules, or MCP tool allowlists from inside `agent run`
- Missing or incorrect audit log records for proxy decisions
- Credentials written to disk, logs, or telemetry where they should not be

Out of scope: attacks that require a malicious or compromised process running as the same user outside the agent sandbox. `agent run` is not designed as a boundary against that; see [Threat Model and Security Boundary](README.md#threat-model-and-security-boundary).
