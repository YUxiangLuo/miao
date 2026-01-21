#!/bin/bash
set -e

cd "$(dirname "$0")"

# 1. Build Frontend
echo "==> 编译前端 (Web Dashboard)..."
if command -v bun &> /dev/null; then
    cd web
    bun install && bun run build
    cd ..
    cp web/dist/index.html public/index.html
    echo "==> 前端构建完成"
else
    echo "警告: 未找到 'bun'，跳过前端构建 (将使用现有的 public/index.html)"
fi

SING_BOX_BIN="embedded/sing-box-amd64"

# 检查 sing-box 是否需要编译
if [ ! -s "$SING_BOX_BIN" ]; then
    echo "==> 编译 sing-box..."

    TMPDIR=$(mktemp -d)
    trap "rm -rf $TMPDIR" EXIT

    git clone --depth=1 https://github.com/SagerNet/sing-box.git "$TMPDIR/sing-box"
    cd "$TMPDIR/sing-box"

    CGO_ENABLED=0 go build -tags "with_quic,with_clash_api" ./cmd/sing-box

    cd - > /dev/null
    cp "$TMPDIR/sing-box/sing-box" "$SING_BOX_BIN"

    echo "==> sing-box 编译完成"
else
    echo "==> sing-box 已存在，跳过"
fi

# 编译 miao-rust
echo "==> 编译 miao-rust (debug)..."
cargo build

echo "==> 完成: target/debug/miao-rust"
