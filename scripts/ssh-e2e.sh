#!/usr/bin/env bash
# End-to-end test of `stashbase ssh` against a throwaway Debian container
# running sshd, through the real `ssh` client.
#
#   scripts/ssh-e2e.sh <stashbase> [<linux stashbase>]
#
# The first binary runs here; the second (default: the first) is copied onto
# the machine after `ssh install`, so the remote side runs this build too.
# `ssh install` itself downloads the latest release, so it needs network
# access to GitHub. Needs docker and ssh-keygen.
set -euo pipefail

local_bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
remote_bin=$(cd "$(dirname "${2:-$1}")" && pwd)/$(basename "${2:-$1}")
work=$(mktemp -d)
container="stashbase-ssh-e2e-$$"
host=e2e

cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

ssh-keygen -q -t ed25519 -N '' -f "$work/id"
docker run -d --platform linux/amd64 --name "$container" -p 127.0.0.1::22 -e PUBKEY="$(cat "$work/id.pub")" \
  debian:13-slim sh -c '
    set -e
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
      openssh-server tmux curl ca-certificates ncurses-bin >/dev/null
    useradd -m -s /bin/bash dev
    mkdir -p /run/sshd /home/dev/.ssh
    echo "$PUBKEY" > /home/dev/.ssh/authorized_keys
    chown -R dev:dev /home/dev/.ssh
    chmod 700 /home/dev/.ssh
    chmod 600 /home/dev/.ssh/authorized_keys
    exec /usr/sbin/sshd -D -e' >/dev/null
port=$(docker port "$container" 22 | head -n 1 | sed 's/.*://')

# The CLI runs plain `ssh`, so a wrapper first on PATH points it at this
# config, leaving the user's own ~/.ssh/config alone.
cat >"$work/config" <<EOF
Host $host
  HostName 127.0.0.1
  Port $port
  User dev
  IdentityFile $work/id
  IdentitiesOnly yes
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
  LogLevel ERROR
EOF
mkdir "$work/bin"
cat >"$work/bin/ssh" <<EOF
#!/bin/sh
exec $(command -v ssh) -F "$work/config" "\$@"
EOF
chmod +x "$work/bin/ssh"
export PATH="$work/bin:$PATH"

echo "Waiting for sshd in $container..."
for _ in $(seq 1 120); do
  ssh -o ConnectTimeout=2 "$host" true 2>/dev/null && break
  sleep 1
done
ssh "$host" true

failures=0
# run <command...>: sets `out` (stdout and stderr) and `status`.
run() {
  set +e
  out=$("$@" 2>&1)
  status=$?
  set -e
}
check() {
  local name=$1 ok=$2
  if [ "$ok" = yes ]; then
    echo "ok - $name"
  else
    echo "not ok - $name (exit $status)"
    printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1))
  fi
}
contains() { case "$out" in *"$1"*) echo yes ;; *) echo no ;; esac }

run "$local_bin" ssh doctor "$host"
check "doctor before install says it isn't installed" \
  "$([ "$status" -ne 0 ] && contains "stashbase is not installed on $host")"

run "$local_bin" ssh install "$host" </dev/null
check "install without a terminal asks for --yes" \
  "$([ "$status" -ne 0 ] && contains "Pass --yes")"

run "$local_bin" ssh install "$host" --yes
check "install --yes installs the latest release" \
  "$([ "$status" -eq 0 ] && contains "Done:")"

# From here on the machine runs this build.
docker exec -u dev "$container" mkdir -p /home/dev/.local/bin
docker cp "$remote_bin" "$container:/home/dev/.local/bin/stashbase"
docker exec "$container" chown dev:dev /home/dev/.local/bin/stashbase

run "$local_bin" ssh setup "$host" </dev/null
check "setup without a terminal refuses" \
  "$([ "$status" -ne 0 ] && contains "needs a terminal")"

run env TERM=stashbase-e2e-term "$local_bin" ssh doctor "$host" </dev/null
check "doctor runs the machine's checks" \
  "$([ "$status" -eq 0 ] && contains "Stashbase CLI Doctor")"
check "doctor reports tmux" "$(contains "✓ tmux")"
check "doctor reports an unknown terminal with its fix" \
  "$(contains "infocmp -x stashbase-e2e-term | ssh $host -- tic -x -")"
check "doctor runs the Docker checks" "$(contains "Docker sandbox")"

run sh -c "printf 'echo e2e-marker-\$((6 * 7))\nexit\n' | '$local_bin' ssh open $host --with none"
check "open --with none gives a shell" "$([ "$status" -eq 0 ] && contains "e2e-marker-42")"

# tmux needs a terminal: `script` gives it one (util-linux and BSD spell it
# differently). The client is killed after a few seconds; the session it
# created stays on the machine. An unknown TERM would stop tmux from
# starting without the fallback to xterm-256color.
if script -qec true /dev/null >/dev/null 2>&1; then
  TERM=stashbase-e2e-term script -qec "'$local_bin' ssh open $host --session e2e-work" /dev/null \
    </dev/null >/dev/null 2>&1 &
else
  TERM=stashbase-e2e-term script -q /dev/null "$local_bin" ssh open "$host" --session e2e-work \
    </dev/null >/dev/null 2>&1 &
fi
client=$!
sleep 8
kill "$client" 2>/dev/null || true
wait "$client" 2>/dev/null || true
run ssh "$host" "tmux list-sessions -F '#{session_name}'"
check "open --session creates the named tmux session" "$(contains "e2e-work")"

run "$local_bin" ssh sessions list "$host"
check "sessions list shows the session open created" \
  "$([ "$status" -eq 0 ] && contains "e2e-work:")"

run "$local_bin" ssh sessions list "$host" --with zellij
check "sessions list says when the multiplexer isn't installed" \
  "$([ "$status" -ne 0 ] && contains "zellij is not installed on $host")"

run "$local_bin" ssh sessions close "$host" e2e-work </dev/null
check "sessions close without a terminal asks for --yes" \
  "$([ "$status" -ne 0 ] && contains "Pass --yes")"

run "$local_bin" ssh sessions close "$host" e2e-work --yes
check "sessions close --yes closes the session" "$([ "$status" -eq 0 ] && contains "Closed session")"
run ssh "$host" "tmux has-session -t =e2e-work"
check "the closed session is gone" "$([ "$status" -ne 0 ] && echo yes)"

run "$local_bin" ssh sessions close "$host" e2e-work --yes
check "sessions close says when there is no such session" \
  "$([ "$status" -ne 0 ] && contains "no tmux session 'e2e-work'")"

run "$local_bin" ssh open "$host" --session 'bad;name'
check "open rejects an invalid session name" \
  "$([ "$status" -ne 0 ] && contains "is not a valid session name")"

run "$local_bin" ssh uninstall "$host" --yes
check "uninstall --yes removes the CLI" "$([ "$status" -eq 0 ] && echo yes)"
run ssh "$host" 'test ! -e ~/.local/bin/stashbase && test ! -e ~/.config/stashbase'
check "uninstall leaves no binary or config behind" "$([ "$status" -eq 0 ] && echo yes)"

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
