#!/bin/bash
set -e

# 确保在项目根目录运行
cd "$(dirname "$0")"

# 1. 编译前端 (Web Dashboard)
echo "==> 正在编译前端..."
cd web
if command -v bun &> /dev/null; then
    echo "  -> 使用 bun 编译"
    bun install && bun run build
elif command -v npm &> /dev/null; then
    echo "  -> 使用 npm 编译"
    npm install && npm run build
else
    echo "❌ 错误: 未找到 bun 或 npm，无法编译前端。请先安装其中之一。"
    exit 1
fi
cd ..

# 2. 同步前端产物到后端静态目录
mkdir -p public
cp web/dist/index.html public/index.html
echo "✅ 前端构建并同步完成"

# 3. 检查 sing-box 二进制文件
SING_BOX_BIN="embedded/sing-box-amd64"
if [ ! -s "$SING_BOX_BIN" ]; then
    echo "==> 正在编译嵌入式 sing-box..."
    TMPDIR=$(mktemp -d)
    trap "rm -rf $TMPDIR" EXIT

    git clone --depth=1 https://github.com/SagerNet/sing-box.git "$TMPDIR/sing-box"
    cd "$TMPDIR/sing-box"
    CGO_ENABLED=0 go build -tags "with_quic,with_clash_api" ./cmd/sing-box
    cd - > /dev/null
    cp "$TMPDIR/sing-box/sing-box" "$SING_BOX_BIN"
    echo "✅ sing-box 编译完成"
else
    echo "ℹ️ sing-box 已存在，跳过编译"
fi

# 4. 编译 miao-rust 后端
echo "==> 正在编译 Rust 后端 (debug)..."
cargo build

echo "---------------------------------------"
echo "🎉 全部构建完成！"
echo "产物路径: target/debug/miao-rust"
echo "运行命令: sudo ./target/debug/miao-rust"
echo "---------------------------------------"