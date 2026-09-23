FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        gnupg \
        bubblewrap \
    && rm -rf /var/lib/apt/lists/*

# Node.js LTS: the runtime most coding-agent CLIs (Claude Code, Codex,
# Cursor's CLI) ship as npm packages and need at container run time.
# Debian bookworm's own nodejs package is too old for these tools, so pull
# the current LTS from NodeSource instead.
RUN curl -fsSL https://deb.nodesource.com/setup_lts.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code @openai/codex

# A persistent volume is mounted here (see docker_sandbox.rs) so login
# state (e.g. Claude Code's ~/.claude, ~/.claude.json) survives across
# runs instead of vanishing with each --rm'd container. World-writable
# because the container may run as an arbitrary host uid (see the --user
# flag added on Linux in docker_run_command) with no matching passwd
# entry, so a named volume freshly created by Docker would otherwise be
# root-owned and unwritable to that uid.
RUN mkdir -p /home/agent && chmod 777 /home/agent

WORKDIR /workspace
