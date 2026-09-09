#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
EMBEDDED_DIR="$ROOT_DIR/embedded"
KERNEL_DIR="$ROOT_DIR/scripts/sing-box"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

if (( $# > 1 )) || [[ -n "${1:-}" && "$1" != --kernel-only ]]; then
  echo "Usage: $0 [--kernel-only]" >&2
  exit 1
fi
build_rules=true
if [[ "${1:-}" == --kernel-only ]]; then build_rules=false; fi

host_goarch=$(go env GOARCH)
case "$host_goarch" in
  amd64|arm64) ;;
  *)
    echo "Unsupported host Go architecture: $host_goarch" >&2
    exit 1
    ;;
esac

# Kernel upgrades are reviewed changes to source.json, independent of Miao
# releases. Rule data still follows upstream; release CI snapshots both refs.
sing_box_ref=$(bun "$KERNEL_DIR/source.mjs" revision)
sing_box_repo=$(bun "$KERNEL_DIR/source.mjs" repository)
go_version=$(bun "$KERNEL_DIR/source.mjs" go_version)
core_version=$(bun "$KERNEL_DIR/source.mjs" version)
build_tags=$(bun "$KERNEL_DIR/source.mjs" build_tags)
if [[ -n "${SING_BOX_REF:-}" && "$SING_BOX_REF" != "$sing_box_ref" ]]; then
  echo "SING_BOX_REF differs from the pinned kernel; update scripts/sing-box/source.json and review the client patch first" >&2
  exit 1
fi
sing_geoip_ref="${SING_GEOIP_REF:-rule-set}"
direct_rules_ref="${DIRECT_RULES_REF:-release}"

target="${MIAO_TARGET:-}"
if [[ -z "$target" ]]; then
  case "$(uname -m)" in
    x86_64|amd64) target=amd64 ;;
    aarch64|arm64) target=arm64 ;;
    *)
      echo "Unsupported host architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
fi

goos=linux
outfile=""
case "$target" in
  amd64)
    goarch=amd64
    outfile="sing-box-amd64"
    ;;
  arm64)
    goarch=arm64
    outfile="sing-box-arm64"
    ;;
  windows-amd64)
    goos=windows
    goarch=amd64
    outfile="sing-box-windows-amd64.exe"
    ;;
  *)
    echo "Usage: MIAO_TARGET must be amd64, arm64, or windows-amd64" >&2
    exit 1
    ;;
esac

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
# Environment values are consumed by native Go on Windows; do not depend on
# Git Bash translating this custom variable's POSIX temporary path.
if [[ "$("${go_command[@]}" env GOOS)" == windows ]]; then
  registry_snapshot=$(cygpath -m "$registry_snapshot")
  profile_manifest=$(cygpath -m "$profile_manifest")
fi
cp "$KERNEL_DIR/tests/miao_context_test.go" "$KERNEL_DIR/tests/miao_registry_test.go" cmd/sing-box/
echo "==> Testing upstream isolation and recording client capabilities..."
MIAO_REGISTRY_SNAPSHOT="$registry_snapshot" MIAO_CAPTURE_REGISTRIES=1 \
  "${go_command[@]}" test -mod=readonly -tags "$build_tags" ./cmd/sing-box -run '^TestMiao' -count=20

echo "==> Building host sing-box ($host_goarch) for rule compilation..."
"${go_command[@]}" build "${build_flags[@]}" -o "$artifacts/sing-box-host" ./cmd/sing-box

echo "==> Applying Miao client profile..."
git apply --check "$KERNEL_DIR/client.patch"
git apply "$KERNEL_DIR/client.patch"
bun "$KERNEL_DIR/prepare-command.mjs" "$TMP_DIR/sing-box"
MIAO_REGISTRY_SNAPSHOT="$registry_snapshot" MIAO_CAPTURE_REGISTRIES=0 MIAO_PROFILE_MANIFEST="$profile_manifest" \
  "${go_command[@]}" test -mod=readonly -tags "$build_tags" ./cmd/miao-kernel -run '^TestMiao' -count=20

echo "==> Building target sing-box ($target: $goos/$goarch)..."
GOARCH="$goarch" GOOS="$goos" CGO_ENABLED=0 \
  "${go_command[@]}" build "${build_flags[@]}" -o "$artifacts/$outfile" ./cmd/miao-kernel

chmod 755 "$artifacts/sing-box-host" "$artifacts/$outfile"
bun "$ROOT_DIR/scripts/pack-kernel.mjs" "$artifacts/$outfile" "$target"

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
fi

# Publish only after both the kernel and rules have built successfully.
cp "$artifacts/"* "$EMBEDDED_DIR/"

echo "==> Embedded resources ready for $target"
ls -lh "$EMBEDDED_DIR/$outfile" "$EMBEDDED_DIR/$outfile.zst"
if [[ "$build_rules" == true ]]; then
  ls -lh "$EMBEDDED_DIR/geoip-cn.srs" "$EMBEDDED_DIR/geosite-geolocation-cn.srs"
fi
