# The official Node.js LTS image already bundles a known-good Node/npm —
# the runtime most coding-agent CLIs (Claude Code, Codex, Cursor's CLI)
# ship as npm packages and need at container run time. This is still a
# Debian base underneath (bookworm-slim variant), so apt-get below works
# the same as it would on `debian:bookworm-slim`; it just skips having to
# add Node ourselves via NodeSource's curl-pipe-bash setup script.
FROM node:22-bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        bubblewrap \
        iptables \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code @openai/codex

# A persistent volume is mounted here (see docker_sandbox.rs) so login
# state (e.g. Claude Code's ~/.claude, ~/.claude.json) survives across
# runs instead of vanishing with each --rm'd container. World-writable
# because the container may run as an arbitrary host uid (see the --user
# flag set on Linux in docker_run_command) with no matching passwd entry,
# so a named volume freshly created by Docker would otherwise be
# root-owned and unwritable to that uid.
RUN mkdir -p /home/agent && chmod 777 /home/agent

# `iptables` above is used by the short-lived network-namespace-holder
# container (see start_netns_holder in docker_sandbox.rs), which uses this
# same image — the actual agent container never runs iptables itself and
# holds no networking capabilities at all.
WORKDIR /workspace
