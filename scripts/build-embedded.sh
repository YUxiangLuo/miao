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

# Default: remote default branch (latest code). Set SING_BOX_REF=vX.Y.Z to pin.

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

mkdir -p "$EMBEDDED_DIR"

if [[ -n "${SING_BOX_REF:-}" ]]; then
  echo "==> Cloning sing-box source ($SING_BOX_REF)..."
  git clone --depth=1 --branch "$SING_BOX_REF" \
    https://github.com/SagerNet/sing-box.git "$TMP_DIR/sing-box"
else
  echo "==> Cloning sing-box source (default branch)..."
  git clone --depth=1 \
    https://github.com/SagerNet/sing-box.git "$TMP_DIR/sing-box"
fi

cd "$TMP_DIR/sing-box"
build_tags="with_quic,with_clash_api,with_utls"
build_flags=(-trimpath -ldflags "-s -w -buildid=" -tags "$build_tags")

echo "==> Building host sing-box ($host_goarch) for rule compilation..."
go build "${build_flags[@]}" -o "$EMBEDDED_DIR/sing-box-host" ./cmd/sing-box

echo "==> Building target sing-box ($target: $goos/$goarch)..."
GOARCH="$goarch" GOOS="$goos" CGO_ENABLED=0 \
  go build "${build_flags[@]}" -o "$EMBEDDED_DIR/$outfile" ./cmd/sing-box

chmod 755 "$EMBEDDED_DIR/sing-box-host" "$EMBEDDED_DIR/$outfile"

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

echo "==> Downloading adblock rule set..."
curl --fail --location --retry 3 \
  -o "$EMBEDDED_DIR/adblock_reject.srs" \
  https://raw.githubusercontent.com/REIJI007/AdBlock_Rule_For_Sing-box/main/adblock_reject.srs

echo "==> Embedded resources ready for $target"
ls -lh "$EMBEDDED_DIR/$outfile" \
  "$EMBEDDED_DIR/geoip-cn.srs" \
  "$EMBEDDED_DIR/geosite-geolocation-cn.srs" \
  "$EMBEDDED_DIR/adblock_reject.srs"
