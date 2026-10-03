# Telemetry

Stashbase collects privacy-preserving telemetry to improve the product. It is on by
default, you are told once, and you can turn it off at any time. It exists to
show whether the free local CLI turns into real protected agent runs, and where
people get stuck.

## What is sent

One event per run of these nine commands only, which show whether people get set up, use
their secrets, adopt the scanner or the Docker backend, and reach a protected agent run:
`setup`, `pull`, `push`, `run`, `secrets schema pull`, `scan install`, `agent init`,
`agent run` and `agent docker build`. Every other command sends nothing, including the rest
of `secrets`, `scan` and `agent docker`.

| Field | Meaning |
|---|---|
| `event` | always `cli_command` |
| `command` | which command ran, one of the nine listed above |
| `outcome` | `ok`, `error` or `aborted` |
| `error_kind` | only on error: `auth`, `network`, `not_found`, `validation` or `other` |
| `duration_ms` | how long the command took |
| `cli_version`, `os`, `arch` | CLI version and platform |
| `is_tty` | whether it ran in an interactive terminal |
| `install_id` | random ID stored on your machine, not linked to your account; it is persistent, so the data is pseudonymous rather than fully anonymous (it is stored in `telemetry.json` in your config directory; deleting that file gives you a new one) |
| `event_id`, `timestamp_ms` | unique event ID and the time the command finished (Unix milliseconds, from your machine's clock) |

`agent run` events also include these once the run has actually started (a run
that fails earlier, for example on a missing profile, has none of them):

| Field | Meaning |
|---|---|
| `profile_source` | `directory`, `global` or `file`; never the path or profile name |
| `remote` | whether it ran as a remote session |
| `sandbox_backend` | `native` or `docker`, the sandbox backend the run used |
| `worktree` | whether the run used a git worktree (from `--worktree` or the profile) |
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

- Inside an agent session, enforced in layers:
  1. `stashbase agent run` sets `STASHBASE_SANDBOX=1` for the agent and everything it runs
     (native, Docker and remote sessions), and the CLI sends nothing when that variable is
     present with any value.
  2. The CLI also stays silent when the standard CA variables (`SSL_CERT_FILE`,
     `CURL_CA_BUNDLE`, `GIT_SSL_CAINFO`, `NODE_EXTRA_CA_CERTS`, `CODEX_CA_CERTIFICATE`) name one
     of the agent proxy's temporary CA files (`stashbase-proxy-ca-*.pem` for local sessions,
     `remote-proxy-*.pem` for remote ones). A harness that strips Stashbase's own variables but
     keeps the normal proxy and CA setup therefore does not switch suppression off.
  3. For local sessions, the agent proxy refuses the telemetry request itself: a 403 for
     `POST /v1/telemetry` on the Stashbase API host, logged as `telemetry_blocked` in the audit
     log, whatever the profile's egress policy allows. A process that removes the marker and
     the CA variables on purpose is stopped here.

  Remote sessions (`agent run --remote`) have only layers 1 and 2, because their traffic is
  intercepted by a server-side proxy this CLI does not control. A process that deliberately
  removes both the marker and the CA variables can still get an event out there, if the
  profile's egress policy allows the Stashbase API host. Closing that needs the server-side
  proxy to refuse `POST /v1/telemetry` on the API host, as the local proxy does.
- In CI.
- When the API URL is not Stashbase's own service. If `STASHBASE_API_URL` (or the build)
  points at a self-hosted or staging server, nothing is sent to it. Events only go to
  Stashbase's own service; setting `STASHBASE_TELEMETRY_URL` is the explicit opt-in to send
  somewhere else, for example a local backend while developing.
- Before the first-run notice has been shown in an interactive terminal.
- When you opt out.

## Opting out

Any one of these:

- `stashbase config telemetry disable`
- `STASHBASE_TELEMETRY=0`
- `DO_NOT_TRACK=1`

`stashbase config telemetry status` shows the current state and why,
`stashbase config telemetry enable` turns it back on, and `stashbase config print` also shows
the current state on its last line. These commands work even if your `config.toml` is
unreadable.

All of them accept `--json` for machine-readable output, colored in a terminal like the other
commands:

```
$ stashbase config telemetry status --json
{
  "enabled": false
}
```

`status`, `enable` and `disable` all print just `{"enabled": true}` or `{"enabled": false}`.
The plain `status` output also explains why telemetry is off.
