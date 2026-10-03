# Telemetry

Stashbase collects privacy-preserving telemetry to improve the product. It is on by
default, you are told once, and you can turn it off at any time. It exists to
show whether the free local CLI turns into real protected agent runs, and where
people get stuck.

## What is sent

One event per run of these seven commands only, which show whether people get set up, use
their secrets and reach a protected agent run: `setup`, `pull`, `push`, `run`,
`secrets schema pull`, `agent init` and `agent run`. Every other command sends nothing,
including the rest of `secrets`.

| Field | Meaning |
|---|---|
| `event` | always `cli_command` |
| `command` | which command ran: `setup`, `pull`, `push`, `run`, `secrets schema pull`, `agent init` or `agent run` |
| `outcome` | `ok`, `error` or `aborted` |
| `error_kind` | only on error: `auth`, `network`, `not_found`, `validation` or `other` |
| `duration_ms` | how long the command took |
| `cli_version`, `os`, `arch` | CLI version and platform |
| `is_tty` | whether it ran in an interactive terminal |
| `install_id` | random ID stored on your machine, not linked to your account; it is persistent, so the data is pseudonymous rather than fully anonymous (`stashbase telemetry reset` generates a new one) |
| `event_id`, `timestamp_ms` | unique event ID and the time the command finished (Unix milliseconds, from your machine's clock) |

`agent run` events also include these once the run has actually started (a run
that fails earlier, for example on a missing profile, has none of them):

| Field | Meaning |
|---|---|
| `profile_source` | `directory`, `global` or `file`; never the path or profile name |
| `remote` | whether it ran as a remote session |
| `sandbox_backend` | `native` or `docker`, the sandbox backend the run used |
| `policy_allow`, `policy_deny`, `policy_block` | how many policy decisions the proxy made (one HTTPS request can produce more than one allow, for the tunnel and for the forwarded request); only for local runs with the audit log on (the default); counts only, never hosts, paths or policy content |

The event is sent once, when the command finishes, by a short-lived background copy of
`stashbase` that the command starts and does not wait for, so telemetry never delays your
command. The background process receives the event on its standard input, has no terminal
access, and gives up and exits after at most 5 seconds. If you look at your process list you
may briefly see a second `stashbase` process; that is the sender. A slow or unreachable
network just means the event is dropped; it never changes the command's result or exit code.

You can see the exact event before anything is sent: set
`STASHBASE_TELEMETRY_DEBUG=1` and it is printed to stderr instead. This works even before the
first-run notice has been shown and never sends anything (the opt-out switches still apply).

## What is never collected

Commands or arguments, flag values, anything after `--`, file paths, repository
names, project or environment names or IDs, secret names, values or
placeholders, hosts or URLs, policy contents, usernames, machine names, error
messages, environment variables, API keys, or IP addresses.

## When nothing is sent

- Inside an agent session. `stashbase agent run` sets `STASHBASE_SANDBOX=1` for the agent and
  everything it runs (native, Docker and remote sessions), so a Stashbase CLI run by an agent
  never sends telemetry, even if the profile's egress policy would allow the API host.
- In CI.
- Before the first-run notice has been shown in an interactive terminal.
- When you opt out.

## Opting out

Any one of these:

- `stashbase telemetry disable`
- `STASHBASE_TELEMETRY=0`
- `DO_NOT_TRACK=1`

`stashbase telemetry status` shows the current state and why,
`stashbase telemetry enable` turns it back on, and `stashbase telemetry reset`
generates a new install ID.
