#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

echo "==> Installing frontend dependencies with Bun..."
bun install --cwd "$ROOT_DIR/frontend" --frozen-lockfile

echo "==> Building Vite + React frontend..."
bun run --cwd "$ROOT_DIR/frontend" build
