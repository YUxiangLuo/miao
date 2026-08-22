#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

# The ignored Rust test writes the complete binding from serde models. The
# regular (non-ignored) companion test fails CI when the tracked file is stale.
cargo test -p miao-core --locked \
  models::typescript_contract::export_typescript_api_bindings \
  -- --ignored --exact

echo "==> Generated frontend-rsbuild/src/types/api.ts"
