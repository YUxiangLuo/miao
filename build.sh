#!/bin/bash
set -e

cd "$(dirname "$0")"

SING_BOX_BIN="embedded/sing-box-amd64"
ADBLOCK_RULE="embedded/adblock_reject.srs"

# 检查 sing-box 是否需要编译
if [ ! -s "$SING_BOX_BIN" ]; then
  echo "==> 编译 sing-box..."

  TMPDIR=$(mktemp -d)
  trap "rm -rf $TMPDIR" EXIT

  git clone --depth=1 https://github.com/SagerNet/sing-box.git "$TMPDIR/sing-box"
  cd "$TMPDIR/sing-box"

  CGO_ENABLED=0 go build -tags "with_quic,with_clash_api" ./cmd/sing-box

  cd - >/dev/null
  cp "$TMPDIR/sing-box/sing-box" "$SING_BOX_BIN"

  echo "==> sing-box 编译完成"
else
  echo "==> sing-box 已存在，跳过"
fi

echo "==> 下载 AdBlock SRS 规则..."
curl -fL -o "$ADBLOCK_RULE" \
  https://raw.githubusercontent.com/REIJI007/AdBlock_Rule_For_Sing-box/main/adblock_reject.srs

echo "==> 构建 Vite + React 前端..."
npm --prefix frontend install
npm --prefix frontend run build

echo "==> 编译 miao-rust (release)..."
cargo build --release

echo "==> 完成: target/release/miao-rust"
