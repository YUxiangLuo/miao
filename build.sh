#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$ROOT_DIR"

if (( $# > 0 )); then
  echo "Usage: $0 (builds the current host architecture only)" >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64|amd64) target=amd64 ;;
  aarch64|arm64) target=arm64 ;;
  *)
    echo "Unsupported host architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

./scripts/build-frontend.sh
MIAO_TARGET="$target" ./scripts/build-embedded.sh

echo "==> Building miao-rust ($target native release)..."
cargo build --release
echo "==> Complete: target/release/miao-rust"
