#!/bin/bash
# 发布脚本 - 自动更新 [workspace.package] 版本并创建 tag
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

# 更新 Cargo.toml 中的版本号（锚定 [workspace.package] 段内的 version 行；
# 段内替换比「第一个 version 行」稳健，不会误伤未来其他段的 version 条目）
sed -i '/^\[workspace\.package\]/,/^\[/ s/^version = ".*"/version = "'"$CARGO_VERSION"'"/' Cargo.toml

# 确认替换生效（段内没找到 version 行时 sed 静默成功但什么都没改）
if ! grep -q '^version = "'"$CARGO_VERSION"'"' Cargo.toml; then
    echo "❌ 未能在 [workspace.package] 段更新版本号，请检查 Cargo.toml"
    exit 1
fi
echo "✅ 已更新 [workspace.package] 版本为 $CARGO_VERSION（全 workspace 同号）"

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
echo "🎉 发布完成！GitHub Actions 将构建 Linux（amd64/arm64 musl）与 Windows（NSIS 安装包）并上传到 Release。"
echo "   查看进度: https://github.com/YUxiangLuo/miao/actions"
