#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
EMBEDDED_DIR="$ROOT_DIR/embedded"
KERNEL_DIR="$ROOT_DIR/scripts/sing-box"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

case "$(uname -s)" in
  Linux) host_goos=linux ;;
  MINGW*|MSYS*|CYGWIN*) host_goos=windows ;;
  *) echo "Linux or Windows (Git Bash) build host required, got: $(uname -s)" >&2; exit 1 ;;
esac
target="${1:?use ./dev.sh kernel [amd64|arm64|windows-amd64|all]}"
shift
case "$target" in
  amd64|arm64|windows-amd64) targets=("$target") ;;
  all) targets=(amd64 arm64) ;;
  *) echo "Unsupported target: $target" >&2; exit 1 ;;
esac
build_rules=true
if [[ "${1:-}" == --kernel-only ]]; then build_rules=false; shift; fi
(( $# == 0 )) || { echo "Unexpected kernel argument: $1" >&2; exit 1; }

native_path() {
  if [[ "$host_goos" == windows ]] && command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s\n' "$1"
  fi
}

host_goarch=$(go env GOHOSTARCH)
case "$host_goarch" in
  amd64|arm64) ;;
  *)
    echo "Unsupported host Go architecture: $host_goarch" >&2
    exit 1
    ;;
esac

# Kernel upgrades are reviewed changes to source.json, independent of Harbor
# releases. Rule data still follows upstream; release builds snapshot both refs.
source_mjs=$(native_path "$KERNEL_DIR/source.mjs")
sing_box_ref=$(bun "$source_mjs" revision)
sing_box_repo=$(bun "$source_mjs" repository)
go_version=$(bun "$source_mjs" go_version)
core_version=$(bun "$source_mjs" version)
build_tags=$(bun "$source_mjs" build_tags)
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
  if [[ -n "${SING_GEOIP_REF:-}" ]]; then
    [[ "$SING_GEOIP_REF" =~ ^[0-9a-f]{40}$ ]] || { echo "Invalid SING_GEOIP_REF: $SING_GEOIP_REF" >&2; exit 1; }
    sing_geoip_ref=$SING_GEOIP_REF
  else
    sing_geoip_ref=$(resolve_ref https://github.com/SagerNet/sing-geoip.git refs/heads/rule-set)
  fi
  if [[ -n "${DIRECT_RULES_REF:-}" ]]; then
    [[ "$DIRECT_RULES_REF" =~ ^[0-9a-f]{40}$ ]] || { echo "Invalid DIRECT_RULES_REF: $DIRECT_RULES_REF" >&2; exit 1; }
    direct_rules_ref=$DIRECT_RULES_REF
  else
    direct_rules_ref=$(resolve_ref https://github.com/Loyalsoldier/v2ray-rules-dat.git refs/heads/release)
  fi
fi

mkdir -p "$TMP_DIR/artifacts" "$EMBEDDED_DIR"
artifacts="$TMP_DIR/artifacts"
echo "==> Fetching pinned sing-box source ($sing_box_ref)..."
git -C "$TMP_DIR" init -q sing-box
git -C "$TMP_DIR/sing-box" config core.autocrlf false
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
registry_snapshot=$(native_path "$TMP_DIR/registries.json")
profile_manifest=$(native_path "$KERNEL_DIR/source.json")
cp "$KERNEL_DIR/tests/harbor_context_test.go" "$KERNEL_DIR/tests/harbor_registry_test.go" cmd/sing-box/
echo "==> Testing upstream isolation and recording client capabilities..."
GOOS="$host_goos" GOARCH="$host_goarch" HARBOR_REGISTRY_SNAPSHOT="$registry_snapshot" HARBOR_CAPTURE_REGISTRIES=1 \
  "${go_command[@]}" test -mod=readonly -tags "$build_tags" ./cmd/sing-box -run '^TestHarbor' -count=20

host_bin="$artifacts/sing-box-host"
if [[ "$host_goos" == windows ]]; then host_bin="$artifacts/sing-box-host.exe"; fi
if [[ "$build_rules" == true ]]; then
  echo "==> Building host sing-box ($host_goos/$host_goarch) for rule compilation..."
  GOOS="$host_goos" GOARCH="$host_goarch" "${go_command[@]}" build "${build_flags[@]}" -o "$(native_path "$host_bin")" ./cmd/sing-box
fi

# Patch a private dependency copy, never the shared Go module cache. Keep the
# replacement relative so temporary checkout paths do not enter build metadata.
echo "==> Applying Shadowsocks AEAD half-close fix..."
ss_module=github.com/sagernet/sing-shadowsocks2
ss_version=$("${go_command[@]}" list -mod=readonly -m -f '{{.Version}}{{if .Replace}} replaced{{end}}' "$ss_module")
if [[ "$ss_version" != v0.2.2-0.20261110092058-52ade8ab7e95 ]]; then
  echo "Shadowsocks dependency changed; review shadowsocks2-halfclose.patch" >&2
  exit 1
fi
ss_dir=$("${go_command[@]}" list -mod=readonly -m -f '{{.Dir}}' "$ss_module")
cp -R "$ss_dir" harbor-shadowsocks2
chmod -R u+w harbor-shadowsocks2
git apply --check --directory=harbor-shadowsocks2 "$KERNEL_DIR/shadowsocks2-halfclose.patch"
git apply --directory=harbor-shadowsocks2 "$KERNEL_DIR/shadowsocks2-halfclose.patch"
"${go_command[@]}" mod edit "-replace=$ss_module@$ss_version=./harbor-shadowsocks2"

echo "==> Applying Harbor client profile..."
git apply --check "$KERNEL_DIR/client.patch"
git apply "$KERNEL_DIR/client.patch"
bun "$(native_path "$KERNEL_DIR/prepare-command.mjs")" "$(native_path "$TMP_DIR/sing-box")"
GOOS="$host_goos" GOARCH="$host_goarch" HARBOR_REGISTRY_SNAPSHOT="$registry_snapshot" HARBOR_CAPTURE_REGISTRIES=0 HARBOR_PROFILE_MANIFEST="$profile_manifest" \
  "${go_command[@]}" test -mod=readonly -tags "$build_tags" ./cmd/harbor-kernel -run '^TestHarbor' -count=20

for target in "${targets[@]}"; do
  case "$target" in
    amd64|arm64)
      target_goos=linux
      target_goarch=$target
      artifact_name="sing-box-$target"
      ;;
    windows-amd64)
      target_goos=windows
      target_goarch=amd64
      artifact_name="sing-box-windows-amd64.exe"
      ;;
  esac
  echo "==> Building kernel ($target_goos/$target_goarch)..."
  GOARCH="$target_goarch" GOOS="$target_goos" CGO_ENABLED=0 \
    "${go_command[@]}" build "${build_flags[@]}" -o "$(native_path "$artifacts/$artifact_name")" ./cmd/harbor-kernel
  chmod 755 "$artifacts/$artifact_name"
  bun "$(native_path "$ROOT_DIR/scripts/lib/pack-kernel.mjs")" "$(native_path "$artifacts/$artifact_name")" "$target"
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

  bun "$(native_path "$ROOT_DIR/scripts/lib/compile-direct-rules.mjs")" "$(native_path "$direct_list")" "$(native_path "$direct_json")"
  "$host_bin" rule-set compile "$(native_path "$direct_json")" \
    -o "$(native_path "$artifacts/geosite-geolocation-cn.srs")"
  bun "$(native_path "$ROOT_DIR/scripts/lib/release-artifacts.mjs")" rules "$(native_path "$artifacts")" "$sing_geoip_ref" "$direct_rules_ref"
fi

# Do not leave the temporary host rule compiler in embedded/.
rm -f "$host_bin"
# Publish only after both the kernel and rules have built successfully.
cp "$artifacts/"* "$EMBEDDED_DIR/"

echo "==> Kernel resources ready: ${targets[*]}"
