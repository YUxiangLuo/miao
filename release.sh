#!/bin/bash
# 发布脚本 - 自动更新 Cargo.toml 版本并创建 tag
# 用法: ./release.sh v0.3.3

set -e

if [ -z "$1" ]; then
    echo "用法: ./release.sh <version>"
    echo "例如: ./release.sh v0.3.3"
    exit 1
fi

VERSION_INPUT=$1
# 移除 possible v prefix to get clean version number for Cargo.toml
CARGO_VERSION=${VERSION_INPUT#v}
# Ensure v prefix for git tag
TAG_VERSION="v$CARGO_VERSION"

# 验证版本格式 (semver)
if ! [[ "$CARGO_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "❌ 版本格式错误: $CARGO_VERSION (应为 x.y.z)"
    exit 1
fi

# 检查工作区是否干净
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "❌ 工作区有未提交的改动，请先提交或 stash"
    exit 1
fi

# 检查 tag 是否已存在
if git tag -l "$TAG_VERSION" | grep -q .; then
    echo "❌ Tag $TAG_VERSION 已存在"
    exit 1
fi

echo "📦 发布版本: $TAG_VERSION (Cargo: $CARGO_VERSION)"

# 更新 Cargo.toml 中的版本号 (仅替换 [package] 段的 version)
sed -i '0,/^version = ".*"/{s/^version = ".*"/version = "'"$CARGO_VERSION"'"/}' Cargo.toml
echo "✅ 已更新 Cargo.toml 版本为 $CARGO_VERSION"

# 验证编译
echo "🔍 验证编译..."
cargo check --quiet
echo "✅ 编译通过"

# 提交更改
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to $TAG_VERSION"
echo "✅ 已提交版本更新"

# 推送代码
git push origin master
echo "✅ 已推送到 master"

# 创建并推送 tag
git tag "$TAG_VERSION"
git push origin "$TAG_VERSION"
echo "✅ 已创建并推送 tag: $TAG_VERSION"

echo ""
echo "🎉 发布完成！GitHub Actions 将自动构建 Release。"
echo "   查看进度: https://github.com/YUxiangLuo/miao/actions"
