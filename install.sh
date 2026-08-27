#!/usr/bin/env bash
# Miao 一键安装：下载最新 release 并注册为 systemd 服务（仅 Linux + systemd）
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/YUxiangLuo/miao/master/install.sh | sudo bash
#   sudo bash install.sh /path/to/miao-binary   # 离线安装本地二进制
#   curl -fsSL https://miao.vesein.dev/install.sh | sudo MIAO_BASE_URL=https://miao.vesein.dev/dl bash
# 重复运行即事务式升级：先下载并校验，最后才短暂停服替换；启动失败自动回滚。
set -euo pipefail

BIN_PATH=/usr/local/bin/miao
CONFIG_DIR=/etc/miao
UNIT_PATH=/etc/systemd/system/miao.service
REPO=YUxiangLuo/miao
LOCAL_BIN="${1:-}"
BASE_URL="${MIAO_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
if [[ -n "${MIAO_BASE_URL:-}" ]]; then
  REMOVE_SH_URL="${MIAO_BASE_URL%/dl}/remove.sh"
else
  REMOVE_SH_URL="https://raw.githubusercontent.com/$REPO/master/remove.sh"
fi

log() { echo "==> $*"; }

if [[ "$(id -u)" -ne 0 ]]; then
  echo "需要 root 权限，请用 sudo 运行：" >&2
  echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/master/install.sh | sudo bash" >&2
  exit 1
fi
if [[ -n "$LOCAL_BIN" && ! -f "$LOCAL_BIN" ]]; then
  echo "本地二进制不存在: $LOCAL_BIN" >&2
  exit 1
fi
if ! command -v systemctl >/dev/null 2>&1; then
  echo "未找到 systemctl：本脚本只支持 systemd 系统（OpenWrt 请直接运行二进制）" >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64|amd64) asset_arch=amd64 ;;
  aarch64|arm64) asset_arch=arm64 ;;
  *)
    echo "不支持的架构: $(uname -m)（仅支持 amd64 / arm64）" >&2
    exit 1
    ;;
esac
asset="miao-rust-linux-$asset_arch"

if command -v curl >/dev/null 2>&1; then
  download() { curl --fail --location --retry 3 --progress-bar "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  download() { wget -q --show-progress "$1" -O "$2"; }
elif [[ -z "$LOCAL_BIN" ]]; then
  echo "需要 curl 或 wget 来下载二进制文件" >&2
  exit 1
fi

work_dir=$(mktemp -d)
staged_bin="${BIN_PATH}.new.$$"
staged_unit="${UNIT_PATH}.new.$$"
cleanup() {
  rm -rf "$work_dir"
  rm -f "$staged_bin" "$staged_unit"
}
trap cleanup EXIT
candidate="$work_dir/$asset"

# 下载和全部校验都在旧服务仍运行时完成，避免网络依赖自身 TUN 时提前断网。
if [[ -n "$LOCAL_BIN" ]]; then
  log "校验本地二进制: $LOCAL_BIN"
  cp "$LOCAL_BIN" "$candidate"
else
  checksum_file="$work_dir/$asset.sha256"
  log "下载最新 release（linux-$asset_arch）..."
  download "$BASE_URL/$asset" "$candidate"
  download "$BASE_URL/$asset.sha256" "$checksum_file"

  if ! command -v sha256sum >/dev/null 2>&1; then
    echo "缺少 sha256sum，无法校验下载文件" >&2
    exit 1
  fi
  expected_sha=$(awk 'NR == 1 { print $1 }' "$checksum_file")
  if [[ ! "$expected_sha" =~ ^[[:xdigit:]]{64}$ ]]; then
    echo "发布校验文件格式无效" >&2
    exit 1
  fi
  actual_sha=$(sha256sum "$candidate" | awk '{ print $1 }')
  if [[ "${actual_sha,,}" != "${expected_sha,,}" ]]; then
    echo "二进制 SHA256 校验失败，已中止" >&2
    exit 1
  fi
fi

if [[ "$(head -c 4 "$candidate")" != $'\x7fELF' ]]; then
  echo "下载/提供的文件不是有效的 ELF 可执行文件，已中止" >&2
  exit 1
fi
chmod 755 "$candidate"
if ! candidate_version=$("$candidate" --version 2>&1); then
  echo "二进制无法在当前系统运行: $candidate_version" >&2
  exit 1
fi
if [[ ! "$candidate_version" =~ ^miao[[:space:]]v[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "二进制版本输出异常: $candidate_version" >&2
  exit 1
fi
log "候选版本校验通过: $candidate_version"

candidate_unit="$work_dir/miao.service"
cat > "$candidate_unit" <<'EOF'
[Unit]
Description=Miao transparent proxy (embedded sing-box)
# 冷启动时优先等待网络管理器完成链路/DHCP，减少第一次订阅刷新与默认路由竞速。
# Wants 而非 Requires：wait-online 超时/失败后仍启动，运行时后台重试继续兜底。
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
# systemctl restart 时 network-online.target 已是 active，不能代表当前网络可用；
# 因此这里只优化冷启动，断网恢复仍由 miao 的后台订阅重试负责。
ExecStart=/usr/local/bin/miao
WorkingDirectory=/etc/miao
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

# 停服前完成旧文件备份；即使后续预检失败，也能立即恢复原服务。
had_old_bin=no
had_old_unit=no
if [[ -f "$BIN_PATH" ]]; then
  had_old_bin=yes
  cp -a "$BIN_PATH" "$work_dir/old-miao"
fi
if [[ -f "$UNIT_PATH" ]]; then
  had_old_unit=yes
  cp -a "$UNIT_PATH" "$work_dir/old-miao.service"
fi

service_was_active=no
service_was_enabled=no
if systemctl is-enabled --quiet miao 2>/dev/null; then
  service_was_enabled=yes
fi
if systemctl is-active --quiet miao 2>/dev/null; then
  service_was_active=yes
  log "停止当前 miao 服务以执行原子升级"
  systemctl stop miao
fi

# 旧 systemd 实例停止后再检查端口；此时命中的才是其他实例/程序。
if command -v ss >/dev/null 2>&1 && ss -tlnH '( sport = :6161 )' | grep -q .; then
  if [[ "$service_was_active" == yes ]]; then
    systemctl start miao || true
  fi
  echo "端口 6161 被其他进程占用，请先停止占用进程" >&2
  exit 1
fi

rollback_install() {
  echo "服务启动失败，正在回滚旧版本..." >&2
  systemctl stop miao 2>/dev/null || true
  if [[ "$had_old_bin" == yes ]]; then
    install -m 755 "$work_dir/old-miao" "$staged_bin"
    mv -f "$staged_bin" "$BIN_PATH"
  else
    rm -f "$BIN_PATH"
  fi
  if [[ "$had_old_unit" == yes ]]; then
    install -m 644 "$work_dir/old-miao.service" "$staged_unit"
    mv -f "$staged_unit" "$UNIT_PATH"
  else
    rm -f "$UNIT_PATH"
  fi
  systemctl daemon-reload || true
  if [[ "$service_was_enabled" == yes && "$had_old_unit" == yes ]]; then
    systemctl enable miao >/dev/null 2>&1 || true
  else
    systemctl disable miao >/dev/null 2>&1 || true
  fi
  if [[ "$service_was_active" == yes ]]; then
    systemctl start miao || echo "旧版本恢复后仍无法启动，请检查 journalctl -u miao -e" >&2
  fi
}

log "原子安装到 $BIN_PATH"
if ! install -m 755 "$candidate" "$staged_bin" \
  || ! mv -f "$staged_bin" "$BIN_PATH" \
  || ! mkdir -p "$CONFIG_DIR" \
  || ! chmod 750 "$CONFIG_DIR" \
  || ! install -m 644 "$candidate_unit" "$staged_unit" \
  || ! mv -f "$staged_unit" "$UNIT_PATH"; then
  rollback_install
  exit 1
fi

log "启用并启动 miao 服务"
if ! systemctl daemon-reload \
  || ! systemctl enable miao >/dev/null \
  || ! systemctl start miao; then
  rollback_install
  exit 1
fi
sleep 1
if ! systemctl is-active --quiet miao; then
  rollback_install
  exit 1
fi

# 自更新机制可能留下 .bak；系统安装健康后不再需要它。
rm -f "${BIN_PATH}.bak"
log "完成！面板地址: http://localhost:6161"
echo
echo "常用命令:"
echo "  systemctl status miao    # 查看状态"
echo "  journalctl -u miao -f    # 查看日志"
echo "  卸载: curl -fsSL $REMOVE_SH_URL | sudo bash"
