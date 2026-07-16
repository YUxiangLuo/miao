#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
EMBEDDED_DIR="$ROOT_DIR/embedded"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

host_goarch=$(go env GOARCH)
case "$host_goarch" in
  amd64|arm64) ;;
  *)
    echo "Unsupported host Go architecture: $host_goarch" >&2
    exit 1
    ;;
esac

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

case "$target" in
  amd64) goarch=amd64 ;;
  arm64) goarch=arm64 ;;
  *)
    echo "Usage: MIAO_TARGET must be amd64 or arm64" >&2
    exit 1
    ;;
esac

mkdir -p "$EMBEDDED_DIR"

echo "==> Cloning sing-box source..."
if [[ -n "${SING_BOX_REF:-}" ]]; then
  git clone --depth=1 --branch "$SING_BOX_REF" \
    https://github.com/SagerNet/sing-box.git "$TMP_DIR/sing-box"
else
  git clone --depth=1 https://github.com/SagerNet/sing-box.git "$TMP_DIR/sing-box"
fi

cd "$TMP_DIR/sing-box"
build_tags="with_quic,with_clash_api,with_utls"
build_flags=(-trimpath -ldflags "-s -w -buildid=" -tags "$build_tags")

echo "==> Building host sing-box ($host_goarch) for rule compilation..."
go build "${build_flags[@]}" -o "$EMBEDDED_DIR/sing-box-host" ./cmd/sing-box

echo "==> Building target sing-box ($target)..."
GOARCH="$goarch" GOOS=linux CGO_ENABLED=0 \
  go build "${build_flags[@]}" -o "$EMBEDDED_DIR/sing-box-$target" ./cmd/sing-box

chmod 755 "$EMBEDDED_DIR/sing-box-host" "$EMBEDDED_DIR/sing-box-$target"

echo "==> Downloading and compiling geo rule files..."
curl --fail --location --retry 3 \
  -o "$EMBEDDED_DIR/geoip-cn.srs" \
  https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs

direct_list="$TMP_DIR/direct-list.txt"
direct_json="$TMP_DIR/direct-list.json"
curl --fail --location --retry 3 \
  -o "$direct_list" \
  https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/direct-list.txt

bun "$ROOT_DIR/scripts/compile-direct-rules.mjs" "$direct_list" "$direct_json"
"$EMBEDDED_DIR/sing-box-host" rule-set compile "$direct_json" \
  -o "$EMBEDDED_DIR/geosite-geolocation-cn.srs"

echo "==> Embedded resources ready for $target"
ls -lh "$EMBEDDED_DIR/sing-box-$target" \
  "$EMBEDDED_DIR/geoip-cn.srs" \
  "$EMBEDDED_DIR/geosite-geolocation-cn.srs"
