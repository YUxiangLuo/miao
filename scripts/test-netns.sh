#!/usr/bin/env bash
# Linux integration smoke test. The application and all network mutations run
# inside new network/mount/PID namespaces; no host service is stopped or installed.
set -euo pipefail

die() { echo "test-netns: $*" >&2; exit 1; }

if [[ "${1:-}" == --inside ]]; then
  shift
  [[ $# == 5 && $EUID == 0 && $$ == 1 ]] || die "internal worker requires a new root PID namespace"
  [[ $(readlink /proc/self/ns/net) != "$4" ]] || die "network namespace was not isolated"
  [[ $(readlink /proc/self/ns/mnt) != "$5" ]] || die "mount namespace was not isolated"

  mount --make-rprivate /
  # Stage inputs before hiding /tmp, so repositories/binaries under /tmp work too.
  # /run is private tmpfs: staging and logs never write into the host's runtime.
  mount -t tmpfs -o size=768m tmpfs /run
  cp -- "$1" /run/miao-rust
  cp -- "$2" /run/bun
  cp -- "$3" /run/netns-smoke.mjs
  mount -t tmpfs -o size=256m tmpfs /tmp
  mount -t sysfs sysfs /sys
  cd /tmp
  unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy

  ip link set lo up
  ip link add test-uplink type dummy
  ip address add 192.0.2.2/24 dev test-uplink
  ip link set test-uplink up
  ip route add default via 192.0.2.1

  cat > /tmp/miao-smoke.yaml <<'YAML'
port: 6161
nodes:
  - '{"type":"trojan","tag":"smoke-node","server":"192.0.2.1","server_port":443,"password":"isolated-test-password","tls":{"enabled":true,"server_name":"example.com"}}'
custom_rules:
  - '{"domain_suffix":"direct.example","outbound":"direct"}'
  - '{"domain_suffix":"pinned.example","outbound":"smoke-node"}'
  - '{"domain_suffix":"tcp.example","network":"tcp","outbound":"smoke-node"}'
  - '{"domain_suffix":"dormant.example","outbound":"gone-node"}'
YAML

  smoke_pid=''
  # shellcheck disable=SC2329 # Called by the EXIT trap inside this worker branch.
  cleanup() {
    local result=$1
    trap - EXIT
    if [[ -n "$smoke_pid" ]]; then
      kill -TERM "$smoke_pid" 2>/dev/null || true
      wait "$smoke_pid" || true
    fi
    if (( result != 0 )); then
      tail -n 80 /tmp/miao-smoke.log >&2
    fi
    exit "$result"
  }
  trap 'cleanup "$?"' EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  /run/miao-rust --config /tmp/miao-smoke.yaml > /tmp/miao-smoke.log 2>&1 &
  smoke_pid=$!
  /run/bun /run/netns-smoke.mjs "$smoke_pid"
  exit 0
fi

if [[ "${1:-}" == --help || "${1:-}" == -h ]]; then
  echo "Usage: ./scripts/test-netns.sh [path-to-miao-rust]"
  echo "Requires Linux, a real embedded kernel, Bun, iproute2, util-linux and root/sudo."
  echo "Build first: cargo build --locked -p miao-cli (with generated assets prepared)."
  echo "Default binary: target/debug/miao-rust. Overall timeout: 120 seconds."
  exit 0
fi
[[ $# -le 1 ]] || die "expected at most one binary path; see --help"
[[ $(uname -s) == Linux ]] || die "Linux namespaces are required"
for tool in unshare mount ip bun timeout readlink realpath; do
  command -v "$tool" >/dev/null || die "missing required tool: $tool"
done

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
binary=$(realpath -e -- "${1:-$SCRIPT_DIR/../target/debug/miao-rust}")
[[ -f "$binary" && -x "$binary" ]] || die "not an executable file: $binary"
bun_binary=$(realpath -e -- "$(command -v bun)")
host_net=$(readlink /proc/self/ns/net)
host_mount=$(readlink /proc/self/ns/mnt)
elevate=()
if (( EUID != 0 )); then
  command -v sudo >/dev/null || die "run as root or install sudo"
  elevate=(sudo --)
fi

echo "==> Isolated kernel smoke test: $binary"
exec "${elevate[@]}" timeout --signal=TERM --kill-after=10s 120s \
  unshare --net --mount --pid --fork --mount-proc --kill-child \
  /bin/bash "$SCRIPT_DIR/test-netns.sh" --inside \
  "$binary" "$bun_binary" "$SCRIPT_DIR/netns-smoke.mjs" "$host_net" "$host_mount"
