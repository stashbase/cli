# Node.js is still needed here even though Claude Code and Codex are
# installed as native binaries below (neither invokes Node itself at
# runtime): MCP servers agents commonly connect to are typically run via
# `npx`, so `npm`/`node` need to be on PATH for those to work. This is a
# Debian base underneath (bookworm-slim variant), so apt-get below works
# the same as it would on `debian:bookworm-slim`.
FROM node:22-bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        gh \
        bubblewrap \
        iptables \
        jq \
        dnsutils \
        unzip \
        less \
        nano \
        procps \
        python3 \
        python3-pip \
        python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Debian's system pip refuses a bare `pip install` (PEP 668,
# "externally-managed-environment") to protect the OS's own Python install
# from being clobbered by unrelated packages. That protection matters far
# less for an ephemeral sandbox container than it does on a real host — the
# base image itself is never mutated at runtime, so there's nothing lasting
# to corrupt — so it's relaxed here for a working `pip install` out of the
# box. `python3-venv` is still included for anyone who wants an isolated
# environment anyway.
ENV PIP_BREAK_SYSTEM_PACKAGES=1

# Claude Code and Codex are installed via their own official native
# installers rather than `npm install -g` — both projects document this as
# the recommended method: it installs a self-contained platform binary
# (no Node runtime involved for the tool itself, unlike the npm package
# which just wraps the same binary), and pulls from Anthropic's/OpenAI's
# own signed release infrastructure rather than the npm registry.
#
# Both installers default to installing under $HOME (`/root` during this
# build) and only add `~/.local/bin` to *root's* shell profile — neither
# knows this container will later run as an arbitrary uid (see the --user
# flag set on Linux in docker_run_command). So: make the installed trees
# world-readable/executable (`chmod -R a+rX`, never +w — nothing here
# should be writable by the sandboxed process) and symlink both into
# `/usr/local/bin`, which is on PATH for every user by default. Verified
# directly that both `claude --version` and `codex --version` work when
# invoked as a non-root uid with this layout, not just as root.
#
# Codex ships helper binaries (a code-mode host, voice tools) alongside
# its main executable, so its install tree is kept intact and symlinked
# rather than copying a single binary out the way Claude Code's simpler,
# single-file layout would allow.
RUN curl -fsSL https://claude.ai/install.sh | bash \
    && curl -fsSL https://chatgpt.com/codex/install.sh | sh \
    && chmod 755 /root \
    && chmod -R a+rX /root/.local /root/.codex \
    && ln -s /root/.local/bin/claude /usr/local/bin/claude \
    && ln -s /root/.local/bin/codex /usr/local/bin/codex

# A persistent volume is mounted here (see docker_sandbox.rs) so login
# state (e.g. Claude Code's ~/.claude, ~/.claude.json) survives across
# runs instead of vanishing with each --rm'd container. World-writable
# because the container may run as an arbitrary host uid (see the --user
# flag set on Linux in docker_run_command) with no matching passwd entry,
# so a named volume freshly created by Docker would otherwise be
# root-owned and unwritable to that uid.
RUN mkdir -p /home/agent && chmod 777 /home/agent

# Point user-level installs (`npm i -g`, `pip install --user`) into the
# persistent home volume so tools installed via `agent docker shell` survive
# across runs; npm's default global prefix (/usr/local) lives in the
# container's --rm'd layer and isn't writable by a non-root uid anyway.
# Appended (not prepended) to PATH so a stale install in the shared home can
# never shadow the image's own binaries (e.g. the native `claude`/`codex`).
ENV NPM_CONFIG_PREFIX=/home/agent/.npm-global
ENV PATH=$PATH:/home/agent/.npm-global/bin:/home/agent/.local/bin

# `iptables` above is used by the short-lived network-namespace-holder
# container (see start_netns_holder in docker_sandbox.rs), which uses this
# same image — the actual agent container never runs iptables itself and
# holds no networking capabilities at all.
WORKDIR /workspace
