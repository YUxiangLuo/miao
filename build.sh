#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$ROOT_DIR"

build_arm64=1
for arg in "$@"; do
  case "$arg" in
    --no-arm64)
      build_arm64=0
      ;;
    -h|--help)
      cat <<'EOF'
Usage: ./build.sh [--no-arm64]

Builds the frontend, the embedded kernels and the Rust release binaries.

  --no-arm64   Build only the native host binary; skip the arm64 musl cross build.

Outputs:
  target/release/miao-rust                          native host binary
  target/aarch64-unknown-linux-musl/release/miao-rust   arm64 musl binary (unless --no-arm64)

The arm64 cross build requires zig and cargo-zigbuild, plus the
aarch64-unknown-linux-musl Rust target.
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $arg (see --help)" >&2
      exit 1
      ;;
  esac
done

case "$(uname -m)" in
  x86_64|amd64) host_target=amd64 ;;
  aarch64|arm64) host_target=arm64 ;;
  *)
    echo "Unsupported host architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

# 先检查 arm64 交叉工具链，避免在长构建结束后才失败。
if (( build_arm64 )) && [[ "$host_target" != arm64 ]]; then
  command -v zig >/dev/null 2>&1 || {
    echo "zig is required for the arm64 build (e.g. sudo pacman -S zig cargo-zigbuild)" >&2
    exit 1
  }
  command -v cargo-zigbuild >/dev/null 2>&1 || {
    echo "cargo-zigbuild is required for the arm64 build (e.g. sudo pacman -S zig cargo-zigbuild)" >&2
    exit 1
  }
  command -v rustup >/dev/null 2>&1 || {
    echo "rustup is required to install the aarch64-unknown-linux-musl target" >&2
    exit 1
  }
  rustup target list --installed | grep -qx aarch64-unknown-linux-musl \
    || rustup target add aarch64-unknown-linux-musl
fi

./scripts/build-frontend.sh

# 规则文件与目标架构无关，只在 host 目标构建时下载编译；arm64 只补内核。
MIAO_TARGET="$host_target" ./scripts/build-embedded.sh
if (( build_arm64 )) && [[ "$host_target" != arm64 ]]; then
  MIAO_TARGET=arm64 ./scripts/build-embedded.sh --kernel-only
fi

echo "==> Building miao-rust ($host_target native release)..."
cargo build --locked --release
echo "    -> target/release/miao-rust"

if (( build_arm64 )) && [[ "$host_target" != arm64 ]]; then
  echo "==> Building miao-rust (arm64 musl release)..."
  cargo zigbuild --locked --release --target aarch64-unknown-linux-musl
  echo "    -> target/aarch64-unknown-linux-musl/release/miao-rust"
fi

echo "==> Complete"
