#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
created_files=$(mktemp)

cleanup() {
  while IFS= read -r path; do rm -f "$path"; done < "$created_files"
  rm -f "$created_files"
}
trap cleanup EXIT

"$ROOT_DIR/scripts/build-frontend.sh"
bun "$ROOT_DIR/scripts/prepare-test-assets.mjs" "$created_files"

echo "==> Running Rust tests (missing assets use inert stubs; no proxy or TUN is started)..."
cargo test --manifest-path "$ROOT_DIR/Cargo.toml" --locked --all-targets
