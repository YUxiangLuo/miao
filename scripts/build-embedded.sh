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

echo "==> Downloading adblock rule set..."
curl --fail --location --retry 3 \
  -o "$EMBEDDED_DIR/adblock_reject.srs" \
  https://raw.githubusercontent.com/REIJI007/AdBlock_Rule_For_Sing-box/main/adblock_reject.srs

echo "==> Downloading GeoIP city database (mmdb, for map mode)..."
# 首选 DB-IP 官方(CC BY 4.0);不可达时回退 GitHub 上的 GeoLite2 镜像
geo_month="$(date +%Y-%m)"
geo_prev_month="$(date -d '-1 month' +%Y-%m 2>/dev/null || date -v-1m +%Y-%m)"
geo_mmdb_tmp="$TMP_DIR/geoip-city.mmdb"
if curl --fail --location --retry 3 \
    -o "$geo_mmdb_tmp.gz" \
    "https://download.db-ip.com/free/dbip-city-lite-${geo_month}.mmdb.gz" || \
   curl --fail --location --retry 3 \
    -o "$geo_mmdb_tmp.gz" \
    "https://download.db-ip.com/free/dbip-city-lite-${geo_prev_month}.mmdb.gz"; then
  gunzip -c "$geo_mmdb_tmp.gz" > "$geo_mmdb_tmp"
else
  echo "DB-IP unreachable, falling back to GeoLite2 mirror..."
  curl --fail --location --retry 3 -C - \
    -o "$geo_mmdb_tmp" \
    https://raw.githubusercontent.com/P3TERX/GeoLite.mmdb/download/GeoLite2-City.mmdb
fi
mv "$geo_mmdb_tmp" "$EMBEDDED_DIR/geoip-city.mmdb"

echo "==> Embedded resources ready for $target"
ls -lh "$EMBEDDED_DIR/sing-box-$target" \
  "$EMBEDDED_DIR/geoip-cn.srs" \
  "$EMBEDDED_DIR/geosite-geolocation-cn.srs" \
  "$EMBEDDED_DIR/adblock_reject.srs" \
  "$EMBEDDED_DIR/geoip-city.mmdb"
