#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
EMBEDDED_DIR="$ROOT_DIR/embedded"
KERNEL_DIR="$ROOT_DIR/scripts/sing-box"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

[[ $(uname -s) == Linux ]] || { echo "Linux build host required" >&2; exit 1; }
target="${1:?use ./dev.sh kernel [amd64|arm64|all]}"
shift
case "$target" in
  amd64|arm64) targets=("$target") ;;
  all) targets=(amd64 arm64) ;;
  *) echo "Unsupported target: $target" >&2; exit 1 ;;
esac
build_rules=true
if [[ "${1:-}" == --kernel-only ]]; then build_rules=false; shift; fi
(( $# == 0 )) || { echo "Unexpected kernel argument: $1" >&2; exit 1; }

host_goarch=$(go env GOARCH)
case "$host_goarch" in
  amd64|arm64) ;;
  *)
    echo "Unsupported host Go architecture: $host_goarch" >&2
    exit 1
    ;;
esac

# Kernel upgrades are reviewed changes to source.json, independent of Miao
# releases. Rule data still follows upstream; release builds snapshot both refs.
sing_box_ref=$(bun "$KERNEL_DIR/source.mjs" revision)
sing_box_repo=$(bun "$KERNEL_DIR/source.mjs" repository)
go_version=$(bun "$KERNEL_DIR/source.mjs" go_version)
core_version=$(bun "$KERNEL_DIR/source.mjs" version)
build_tags=$(bun "$KERNEL_DIR/source.mjs" build_tags)
if [[ -n "${SING_BOX_REF:-}" && "$SING_BOX_REF" != "$sing_box_ref" ]]; then
  echo "SING_BOX_REF differs from the pinned kernel; update scripts/sing-box/source.json and review the client patch first" >&2
  exit 1
fi
resolve_ref() {
  local sha ref
  while IFS=$'\t' read -r sha ref; do
    if [[ "$ref" == "$2" && "$sha" =~ ^[0-9a-f]{40}$ ]]; then
      printf '%s\n' "$sha"
      return 0
    fi
  done < <(git ls-remote --exit-code "$1" "$2")
  return 1
}
if [[ "$build_rules" == true ]]; then
  sing_geoip_ref=$(resolve_ref https://github.com/SagerNet/sing-geoip.git refs/heads/rule-set)
  direct_rules_ref=$(resolve_ref https://github.com/Loyalsoldier/v2ray-rules-dat.git refs/heads/release)
fi

mkdir -p "$TMP_DIR/artifacts" "$EMBEDDED_DIR"
artifacts="$TMP_DIR/artifacts"
echo "==> Fetching pinned sing-box source ($sing_box_ref)..."
git -C "$TMP_DIR" init -q sing-box
git -C "$TMP_DIR/sing-box" remote add origin "$sing_box_repo"
for attempt in 1 2 3; do
  if git -C "$TMP_DIR/sing-box" fetch -q --depth=1 origin "$sing_box_ref"; then
    break
  fi
  if (( attempt == 3 )); then exit 1; fi
  echo "Retrying pinned source fetch ($attempt/3)..." >&2
  sleep "$attempt"
done
git -C "$TMP_DIR/sing-box" checkout -q FETCH_HEAD

cd "$TMP_DIR/sing-box"
# Always select the reviewed toolchain, even when the host Go is newer.
go_command=(env "GOTOOLCHAIN=go$go_version" CGO_ENABLED=0 go)
actual_go=$("${go_command[@]}" env GOVERSION)
if [[ "$actual_go" != "go$go_version" ]]; then
  echo "Expected go$go_version, got $actual_go" >&2
  exit 1
fi
build_flags=(-mod=readonly -trimpath -ldflags "-s -w -buildid= -X github.com/sagernet/sing-box/constant.Version=$core_version" -tags "$build_tags")

# The upstream isolation fix is now part of the pin. Keep its behavioral
# tests separately, and capture upstream support before applying our profile.
registry_snapshot="$TMP_DIR/registries.json"
profile_manifest="$KERNEL_DIR/source.json"
cp "$KERNEL_DIR/tests/miao_context_test.go" "$KERNEL_DIR/tests/miao_registry_test.go" cmd/sing-box/
echo "==> Testing upstream isolation and recording client capabilities..."
MIAO_REGISTRY_SNAPSHOT="$registry_snapshot" MIAO_CAPTURE_REGISTRIES=1 \
  "${go_command[@]}" test -mod=readonly -tags "$build_tags" ./cmd/sing-box -run '^TestMiao' -count=20

if [[ "$build_rules" == true ]]; then
  echo "==> Building host sing-box ($host_goarch) for rule compilation..."
  "${go_command[@]}" build "${build_flags[@]}" -o "$artifacts/sing-box-host" ./cmd/sing-box
fi

# Patch a private dependency copy, never the shared Go module cache. Keep the
# replacement relative so temporary checkout paths do not enter build metadata.
echo "==> Applying Shadowsocks AEAD half-close fix..."
ss_module=github.com/sagernet/sing-shadowsocks2
ss_version=$("${go_command[@]}" list -mod=readonly -m -f '{{.Version}}{{if .Replace}} replaced{{end}}' "$ss_module")
if [[ "$ss_version" != v0.2.2-0.20260929152114-a69d1086332b ]]; then
  echo "Shadowsocks dependency changed; review shadowsocks2-halfclose.patch" >&2
  exit 1
fi
ss_dir=$("${go_command[@]}" list -mod=readonly -m -f '{{.Dir}}' "$ss_module")
cp -R "$ss_dir" miao-shadowsocks2
chmod -R u+w miao-shadowsocks2
git apply --check --directory=miao-shadowsocks2 "$KERNEL_DIR/shadowsocks2-halfclose.patch"
git apply --directory=miao-shadowsocks2 "$KERNEL_DIR/shadowsocks2-halfclose.patch"
"${go_command[@]}" mod edit "-replace=$ss_module@$ss_version=./miao-shadowsocks2"

echo "==> Applying Miao client profile..."
git apply --check "$KERNEL_DIR/client.patch"
git apply "$KERNEL_DIR/client.patch"
bun "$KERNEL_DIR/prepare-command.mjs" "$TMP_DIR/sing-box"
MIAO_REGISTRY_SNAPSHOT="$registry_snapshot" MIAO_CAPTURE_REGISTRIES=0 MIAO_PROFILE_MANIFEST="$profile_manifest" \
  "${go_command[@]}" test -mod=readonly -tags "$build_tags" ./cmd/miao-kernel -run '^TestMiao' -count=20

for target in "${targets[@]}"; do
  echo "==> Building kernel (linux/$target)..."
  GOARCH="$target" GOOS=linux CGO_ENABLED=0 \
    "${go_command[@]}" build "${build_flags[@]}" -o "$artifacts/sing-box-$target" ./cmd/miao-kernel
  chmod 755 "$artifacts/sing-box-$target"
  bun "$ROOT_DIR/scripts/pack-kernel.mjs" "$artifacts/sing-box-$target" "$target"
done
if [[ "$build_rules" == true ]]; then
  echo "==> Downloading and compiling geo rule files..."
  curl --fail --location --retry 3 \
    -o "$artifacts/geoip-cn.srs" \
    "https://raw.githubusercontent.com/SagerNet/sing-geoip/${sing_geoip_ref}/geoip-cn.srs"

  direct_list="$TMP_DIR/direct-list.txt"
  direct_json="$TMP_DIR/direct-list.json"
  curl --fail --location --retry 3 \
    -o "$direct_list" \
    "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/${direct_rules_ref}/direct-list.txt"

  bun "$ROOT_DIR/scripts/compile-direct-rules.mjs" "$direct_list" "$direct_json"
  "$artifacts/sing-box-host" rule-set compile "$direct_json" \
    -o "$artifacts/geosite-geolocation-cn.srs"
  bun "$ROOT_DIR/scripts/release-artifacts.mjs" rules "$artifacts" "$sing_geoip_ref" "$direct_rules_ref"
fi

# Publish only after both the kernel and rules have built successfully.
cp "$artifacts/"* "$EMBEDDED_DIR/"

echo "==> Kernel resources ready: ${targets[*]}"
