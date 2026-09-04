#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
EMBEDDED_DIR="$ROOT_DIR/embedded"
created_files=()

cleanup() {
  if (( ${#created_files[@]} > 0 )); then
    rm -f "${created_files[@]}"
  fi
}
trap cleanup EXIT

create_shell_stub() {
  local path="$1"
  if [[ -e "$path" ]]; then
    return
  fi
  printf '#!/bin/sh\necho "inert test-only sing-box" >&2\nexit 1\n' > "$path"
  chmod 755 "$path"
  created_files+=("$path")
}

create_empty_stub() {
  local path="$1"
  if [[ -e "$path" ]]; then
    return
  fi
  : > "$path"
  created_files+=("$path")
}

"$ROOT_DIR/scripts/build-frontend.sh"
mkdir -p "$EMBEDDED_DIR"
create_shell_stub "$EMBEDDED_DIR/sing-box-amd64"
create_shell_stub "$EMBEDDED_DIR/sing-box-arm64"
create_empty_stub "$EMBEDDED_DIR/sing-box-windows-amd64.exe"
create_empty_stub "$EMBEDDED_DIR/geoip-cn.srs"
create_empty_stub "$EMBEDDED_DIR/geosite-geolocation-cn.srs"

echo "==> Running Rust tests with inert embedded assets (no proxy or TUN is started)..."
cargo test --manifest-path "$ROOT_DIR/Cargo.toml" --locked --all-targets
