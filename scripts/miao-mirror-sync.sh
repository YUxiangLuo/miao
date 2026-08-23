#!/usr/bin/env bash
# Miao 镜像站同步:从 GitHub 拉取最新 release 二进制与安装/卸载脚本
# 部署于 root@miao.vesein.dev,由 /etc/cron.d/miao-mirror 每 6 小时调用
set -euo pipefail

DL=/var/www/miao/dl
REPO=YUxiangLuo/miao
WIN_EXE=miao-windows-amd64-setup.exe

mkdir -p "$DL"

latest=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep -oP '"tag_name"\s*:\s*"\K[^"]+')
current=$(cat "$DL/VERSION" 2>/dev/null || echo none)

# 任一发布文件缺失时都全量同步(兼容旧镜像布局)
if [[ "$latest" != "$current" || ! -f "$DL/$WIN_EXE" || ! -f "$DL/miao-rust-linux-amd64.sha256" ]]; then
  echo "new release: $latest (current: $current)"
  # install.sh 要求逐资产 <name>.sha256(与 GitHub release 一致);下载后先本地校验再发布
  for f in miao-rust-linux-amd64 miao-rust-linux-arm64 "$WIN_EXE"; do
    curl -fsSL --retry 3 "https://github.com/$REPO/releases/latest/download/$f" -o "$DL/$f.tmp"
    curl -fsSL --retry 3 "https://github.com/$REPO/releases/latest/download/$f.sha256" -o "$DL/$f.sha256.tmp"
    # 用 release 自带的校验值验证下载完整性(强于魔数检查,损坏则中止并保留旧版本)
    expected=$(awk 'NR == 1 { print $1 }' "$DL/$f.sha256.tmp")
    if [[ ! "$expected" =~ ^[[:xdigit:]]{64}$ ]]; then
      echo "$f.sha256 格式无效,中止(保留旧版本)" >&2
      rm -f "$DL/$f.tmp" "$DL/$f.sha256.tmp"
      exit 1
    fi
    actual=$(sha256sum "$DL/$f.tmp" | awk '{ print $1 }')
    if [[ "${actual,,}" != "${expected,,}" ]]; then
      echo "$f 校验失败,中止(保留旧版本)" >&2
      rm -f "$DL/$f.tmp" "$DL/$f.sha256.tmp"
      exit 1
    fi
    if [[ "$f" == "$WIN_EXE" ]]; then
      chmod 644 "$DL/$f.tmp"
    else
      # ELF 校验:挡错误页/空文件
      if [[ "$(head -c 4 "$DL/$f.tmp")" != $'\x7fELF' ]]; then
        echo "$f 不是有效 ELF,中止(保留旧版本)" >&2
        rm -f "$DL/$f.tmp" "$DL/$f.sha256.tmp"
        exit 1
      fi
      chmod 755 "$DL/$f.tmp"
    fi
    chmod 644 "$DL/$f.sha256.tmp"
    mv "$DL/$f" "$DL/$f.bak.$$" 2>/dev/null || true
    mv "$DL/$f.tmp" "$DL/$f"
    mv "$DL/$f.sha256.tmp" "$DL/$f.sha256"
    rm -f "$DL/$f.bak.$$"
  done
  # 兼容旧文件:install.sh 已改用逐资产 .sha256,sha256sums.txt 仅作历史保留
  (cd "$DL" && sha256sum miao-rust-linux-* "$WIN_EXE" > sha256sums.txt)
  echo "$latest" > "$DL/VERSION"
  echo "updated to $latest"
else
  echo "already latest: $current"
fi

# 安装/卸载脚本跟随 master,每次强制刷新(文本小)
for s in install.sh remove.sh; do
  curl -fsSL "https://raw.githubusercontent.com/$REPO/master/$s" -o "$DL/$s.tmp" && mv "$DL/$s.tmp" "$DL/$s"
done
chmod 644 "$DL"/*.sh
