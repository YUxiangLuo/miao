#!/usr/bin/env bash
# Real reqwest -> authenticated HTTP CONNECT -> Shadowsocks -> local TLS/SSE.
# No production API, TUN, credentials or external provider calls.
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || {
  echo 'This isolated transport test currently requires Linux amd64' >&2
  exit 1
}
for tool in cargo bun jq openssl sudo unshare ip timeout; do command -v "$tool" >/dev/null; done
[[ -x "$ROOT/embedded/sing-box-amd64" && -x "$ROOT/embedded/sing-box-host" ]]
# shellcheck disable=SC2016 # JavaScript template literals, not shell variables.
bun --eval '
  const root = process.argv[1];
  const {readSource, customizationHash} = await import(`${root}/scripts/sing-box/source.mjs`);
  const manifest = await Bun.file(`${root}/embedded/sing-box-amd64.meta.json`).json();
  const bytes = await Bun.file(`${root}/embedded/sing-box-amd64`).arrayBuffer();
  const hash = new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
  if (manifest.version !== readSource().version || manifest.customization_sha256 !== customizationHash()
      || manifest.binary.sha256 !== hash) {
    throw new Error("Rebuild the current kernel first: ./scripts/build-embedded.sh --kernel-only");
  }
' "$ROOT"
binary=$(cargo test --manifest-path "$ROOT/Cargo.toml" -p miao-core --lib \
  --locked --no-run --message-format=json | jq -r 'select(.reason == "compiler-artifact" and .target.name == "miao_core" and .executable != null) | .executable')
[[ -x "$binary" ]]
exec sudo env MIAO_TEST_HOST_NETNS="$(readlink /proc/self/ns/net)" \
  unshare --net --pid --fork --mount --mount-proc bash -c '
    set -euo pipefail
    [[ $(readlink /proc/self/ns/net) != "$MIAO_TEST_HOST_NETNS" ]]
    ip link set lo up
    timeout 120s "$1" --ignored --exact services::ai::fixture::ai_transport_contract --nocapture
    exec timeout 120s "$1" --ignored --exact services::ai::e2e::gateway_contract --nocapture
  ' _ "$binary"
